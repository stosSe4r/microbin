//! Sequential, retryable file staging. Completed files enter the normal create flow.
use crate::{args::ARGS, pasta::PastaFile};
use actix_web::{error, web, Error, HttpResponse};
use rand::Rng;
use serde::Deserialize;
use std::{
    collections::HashMap,
    fs,
    io::{self, Seek, SeekFrom, Write},
    path::PathBuf,
    sync::Mutex,
    time::{Duration, Instant},
};
use tempfile::NamedTempFile;

pub const CHUNK_SIZE: usize = 16 * 1024 * 1024;
const IDLE_TIMEOUT: Duration = Duration::from_secs(60 * 60);

struct Upload {
    file: NamedTempFile,
    metadata: PastaFile,
    expected: u64,
    received: u64,
    touched: Instant,
}

pub struct ChunkUploads {
    directory: PathBuf,
    // ponytail: disk writes share this lock; use per-upload locks if concurrency matters.
    uploads: Mutex<HashMap<String, Upload>>,
}

impl ChunkUploads {
    pub fn new() -> io::Result<Self> {
        let directory = PathBuf::from(&ARGS.data_dir).join(".uploads");
        // Staging is deliberately temporary: a restart cancels unfinished uploads.
        match fs::remove_dir_all(&directory) {
            Ok(()) => (),
            Err(e) if e.kind() == io::ErrorKind::NotFound => (),
            Err(e) => return Err(e),
        }
        fs::create_dir_all(&directory)?;
        Ok(Self {
            directory,
            uploads: Mutex::new(HashMap::new()),
        })
    }

    pub fn temporary(&self) -> io::Result<NamedTempFile> {
        NamedTempFile::new_in(&self.directory)
    }

    pub fn cleanup(&self) {
        self.uploads.lock().unwrap()
            .retain(|_, upload| upload.touched.elapsed() < IDLE_TIMEOUT);
    }

    pub fn take(&self, token: &str) -> io::Result<(PastaFile, NamedTempFile)> {
        let mut uploads = self.uploads.lock().unwrap();
        let upload = uploads.get(token).ok_or_else(|| invalid("上传已过期，请重新选择文件"))?;
        if upload.received != upload.expected {
            return Err(invalid("文件尚未上传完整"));
        }
        let mut upload = uploads.remove(token).unwrap();
        upload.metadata.size = bytesize::ByteSize::b(upload.received);
        Ok((upload.metadata, upload.file))
    }
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

fn http_error(error: io::Error) -> Error {
    if error.kind() == io::ErrorKind::InvalidInput {
        error::ErrorBadRequest(error)
    } else {
        log::error!("Chunk upload failed: {error}");
        error::ErrorInternalServerError("文件写入失败，请重试")
    }
}

#[derive(Deserialize)]
pub struct StartUpload {
    name: String,
    size: u64,
}

pub async fn start(
    data: web::Data<ChunkUploads>,
    input: web::Json<StartUpload>,
) -> Result<HttpResponse, Error> {
    if ARGS.no_file_upload {
        return Err(error::ErrorForbidden("本站未启用文件上传"));
    }
    if input.size > ARGS.max_file_size_unencrypted_mb as u64 * 1024 * 1024 {
        return Err(error::ErrorPayloadTooLarge("文件超过站点大小限制"));
    }
    let metadata = PastaFile::from_unsanitized(&input.name).map_err(error::ErrorBadRequest)?;
    let expected = input.size;
    let id = web::block(move || -> io::Result<String> {
        data.cleanup();
        let file = NamedTempFile::new_in(&data.directory)?;
        let id = format!("{:032x}", rand::thread_rng().gen::<u128>());
        data.uploads.lock().unwrap().insert(id.clone(), Upload {
            file, metadata, expected, received: 0, touched: Instant::now(),
        });
        Ok(id)
    }).await?.map_err(http_error)?;
    Ok(HttpResponse::Ok().insert_header(("Cache-Control", "no-store")).json(serde_json::json!({"id": id})))
}

pub async fn append(
    data: web::Data<ChunkUploads>,
    path: web::Path<(String, u64)>,
    body: web::Bytes,
) -> Result<HttpResponse, Error> {
    let (id, offset) = path.into_inner();
    let received = web::block(move || -> io::Result<u64> {
        let mut uploads = data.uploads.lock().unwrap();
        let upload = uploads.get_mut(&id).ok_or_else(|| invalid("上传已过期，请重试"))?;
        let end = offset.checked_add(body.len() as u64).ok_or_else(|| invalid("分片位置无效"))?;
        // A lost response may repeat the last chunk. Rewrite it, without appending twice.
        if end > upload.expected
            || (offset != upload.received && end != upload.received)
            || body.is_empty()
        {
            return Err(invalid("分片位置或大小不匹配"));
        }
        upload.file.seek(SeekFrom::Start(offset))?;
        upload.file.write_all(&body)?;
        upload.received = end;
        upload.touched = Instant::now();
        Ok(end)
    }).await?.map_err(http_error)?;
    Ok(HttpResponse::Ok().insert_header(("Cache-Control", "no-store")).json(serde_json::json!({"received": received})))
}

pub async fn cancel(
    data: web::Data<ChunkUploads>,
    id: web::Path<String>,
) -> Result<HttpResponse, Error> {
    web::block(move || { data.uploads.lock().unwrap().remove(id.as_str()); }).await?;
    Ok(HttpResponse::NoContent().insert_header(("Cache-Control", "no-store")).finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_uploads_are_removed_without_touching_active_files() {
        let directory = tempfile::tempdir().unwrap();
        let data = ChunkUploads {
            directory: directory.path().to_path_buf(),
            uploads: Mutex::new(HashMap::new()),
        };
        let mut paths = Vec::new();
        for (id, age) in [("expired", IDLE_TIMEOUT), ("active", Duration::ZERO)] {
            let file = data.temporary().unwrap();
            paths.push(file.path().to_path_buf());
            data.uploads.lock().unwrap().insert(id.to_owned(), Upload {
                file,
                metadata: PastaFile::from_unsanitized("file.bin").unwrap(),
                expected: 1,
                received: 0,
                touched: Instant::now() - age,
            });
        }
        data.cleanup();
        assert!(!paths[0].exists());
        assert!(paths[1].exists());
        assert_eq!(data.uploads.lock().unwrap().len(), 1);
        drop(data);
        assert!(!paths[1].exists());
    }
}
