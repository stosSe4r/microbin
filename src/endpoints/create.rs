use crate::pasta::{Pasta, PastaFile};
use crate::util::animalnumbers::to_animal_names;
use crate::util::db::insert;
use crate::util::hashids::to_hashids;
use crate::util::misc::{encrypt, encrypt_file, is_valid_url};
use crate::args::{Args, ARGS};
use crate::AppState;
use actix_multipart::Multipart;
use actix_web::error::ErrorBadRequest;
use actix_web::cookie::Cookie;
use actix_web::{get, web, Error, HttpResponse, Responder};
use askama::Template;
use bytes::BytesMut;
use bytesize::ByteSize;
use futures::TryStreamExt;
use log::warn;
use rand::Rng;
use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Template)]
#[template(path = "index.html")]
struct IndexTemplate<'a> {
    args: &'a Args,
    status: String,
    default_privacy_value: String,
    max_expiry_index: usize,
}

#[get("/")]
pub async fn index() -> impl Responder {
    HttpResponse::Ok().content_type("text/html; charset=utf-8").body(
        IndexTemplate {
            args: &ARGS,
            status: String::from(""),
            default_privacy_value: ARGS.default_privacy.as_ref().map_or_else(|| String::from("public"), |s| s.clone()),
            max_expiry_index: ARGS.max_expiry_index(),
        }
        .render()
        .unwrap(),
    )
}

#[get("/{status:incorrect}")]
pub async fn index_with_status(param: web::Path<String>) -> HttpResponse {
    let status = param.into_inner();

    return HttpResponse::Ok().content_type("text/html; charset=utf-8").body(
        IndexTemplate {
            args: &ARGS,
            status,
            default_privacy_value: ARGS.default_privacy.as_ref().map_or_else(|| String::from("public"), |s| s.clone()),
            max_expiry_index: ARGS.max_expiry_index(),
        }
        .render()
        .unwrap(),
    );
}

const EXPIRATION_OPTIONS: &[&str] = &[
    "1min",
    "10min",
    "1hour",
    "24hour",
    "3days",
    "1week",
    "1month",
    "6months",
    "1year",
    "2years",
    "4years",
    "8years",
    "16years",
    "never",
];

pub fn expiration_to_timestamp(expiration: &str, timenow: i64) -> i64 {
    match expiration {
        "1min" => timenow + 60,
        "10min" => timenow + 60 * 10,
        "1hour" => timenow + 60 * 60,
        "24hour" => timenow + 60 * 60 * 24,
        "3days" => timenow + 60 * 60 * 24 * 3,
        "1week" => timenow + 60 * 60 * 24 * 7,
        "1month" => timenow + 60 * 60 * 24 * 30,
        "6months" => timenow + 60 * 60 * 24 * 30 * 6,
        "1year" => timenow + 60 * 60 * 24 * 365,
        "2years" => timenow + 60 * 60 * 24 * 365 * 2,
        "4years" => timenow + 60 * 60 * 24 * 365 * 4,
        "8years" => timenow + 60 * 60 * 24 * 365 * 8,
        "16years" => timenow + 60 * 60 * 24 * 365 * 16,
        "never" => {
            if ARGS.eternal_pasta {
                0
            } else {
                timenow + 60 * 60 * 24 * 7
            }
        }
        _ => {
            log::error!("{}", "Unexpected expiration time!");
            timenow + 60 * 60 * 24 * 7
        }
    }
}

pub fn is_valid_expiration(expiration: &str, max_expiry: &str) -> bool {
    let max_index = EXPIRATION_OPTIONS.iter().position(|&x| x == max_expiry).unwrap_or(5);
    let current_index = EXPIRATION_OPTIONS.iter().position(|&x| x == expiration).unwrap_or(0);
    current_index <= max_index
}

pub async fn create(
    data: web::Data<AppState>,
    mut payload: Multipart,
    chunks: web::Data<super::chunks::ChunkUploads>,
) -> Result<HttpResponse, Error> {
    let timenow: i64 = match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(n) => n.as_secs(),
        Err(_) => {
            log::error!("SystemTime before UNIX EPOCH!");
            0
        }
    } as i64;

    let mut new_pasta = Pasta {
        id: rand::thread_rng().gen::<u16>() as u64,
        content: String::from(""),
        file: None,
        extension: String::from(""),
        private: false,
        readonly: false,
        editable: ARGS.editable,
        encrypt_server: false,
        encrypted_key: Some(String::from("")),
        encrypt_client: false,
        created: timenow,
        read_count: 0,
        burn_after_reads: 0,
        last_read: timenow,
        pasta_type: String::from(""),
        expiration: expiration_to_timestamp(&ARGS.default_expiry, timenow),
        attachments: None,
    };

    let mut staged_files = Vec::<(PastaFile, tempfile::NamedTempFile)>::new();
    let mut random_key: String = String::from("");
    let mut plain_key: String = String::from("");
    let mut uploader_password = String::from("");

    while let Some(mut field) = payload.try_next().await? {
        let Some(field_name) = field.name() else {
            continue;
        };
        match field_name {
            "uploader_password" => {
                while let Some(chunk) = field.try_next().await? {
                    uploader_password
                        .push_str(std::str::from_utf8(&chunk).unwrap().to_string().as_str());
                }
                continue;
            }
            "random_key" => {
                while let Some(chunk) = field.try_next().await? {
                    random_key = std::str::from_utf8(&chunk).unwrap().to_string();
                }
                continue;
            }
            "privacy" => {
                while let Some(chunk) = field.try_next().await? {
                    let privacy = std::str::from_utf8(&chunk).unwrap();
                    new_pasta.private = match privacy {
                        "public" => false,
                        _ => true,
                    };
                    new_pasta.readonly = match privacy {
                        "readonly" if ARGS.enable_readonly => true,
                        _ => false,
                    };
                    new_pasta.encrypt_client = match privacy {
                        "secret" => true,
                        _ => false,
                    };
                    new_pasta.encrypt_server = match privacy {
                        "private" => true,
                        "secret" => true,
                        _ => false,
                    };
                }
            }
            "plain_key" => {
                while let Some(chunk) = field.try_next().await? {
                    plain_key = std::str::from_utf8(&chunk).unwrap().to_string();
                }
                continue;
            }
            "encrypted_random_key" => {
                while let Some(chunk) = field.try_next().await? {
                    new_pasta.encrypted_key =
                        Some(std::str::from_utf8(&chunk).unwrap().to_string());
                }
                continue;
            }
            "expiration" => {
                let mut expiration_str = String::new();
                while let Some(chunk) = field.try_next().await? {
                    expiration_str = std::str::from_utf8(&chunk).unwrap().to_string();
                }

                if !is_valid_expiration(&expiration_str, &ARGS.max_expiry) {
                    return Err(ErrorBadRequest("Expiration exceeds maximum allowed"));
                }

                new_pasta.expiration = expiration_to_timestamp(&expiration_str, timenow);
                continue;
            }
            "burn_after" => {
                while let Some(chunk) = field.try_next().await? {
                    new_pasta.burn_after_reads = match std::str::from_utf8(&chunk).unwrap() {
                        "1" => 1,
                        "10" => 10,
                        "100" => 100,
                        "1000" => 1000,
                        "10000" => 10000,
                        "0" => 0,
                        _ => {
                            log::error!("{}", "Unexpected burn after value!");
                            0
                        }
                    };
                }

                continue;
            }
            "content" => {
                let mut buf = BytesMut::new();
                while let Some(chunk) = field.try_next().await? {
                    buf.extend_from_slice(&chunk);
                }
                if !buf.is_empty() {
                    new_pasta.content = String::from_utf8(buf.to_vec())
                        .map_err(|_| ErrorBadRequest("Invalid UTF-8 in content"))?;
                    new_pasta.pasta_type = if is_valid_url(new_pasta.content.as_str()) {
                        String::from("url")
                    } else {
                        String::from("text")
                    };
                }
                continue;
            }
            "syntax_highlight" => {
                while let Some(chunk) = field.try_next().await? {
                    new_pasta.extension = std::str::from_utf8(&chunk).unwrap().to_string();
                }
                continue;
            }
            "file" => {
                if ARGS.no_file_upload {
                    continue;
                }

                let path = field.content_disposition().and_then(|cd| cd.get_filename());

                let path = match path {
                    Some("") => continue,
                    Some(p) => p,
                    None => continue,
                };

                let mut file = match PastaFile::from_unsanitized(path) {
                    Ok(f) => f,
                    Err(e) => {
                        warn!("Unsafe file name: {e:?}");
                        continue;
                    }
                };

                let chunks = chunks.clone();
                let mut f = web::block(move || chunks.temporary()).await??;
                let mut size = 0;
                while let Some(chunk) = field.try_next().await? {
                    size += chunk.len();
                    if (new_pasta.encrypt_server
                        && size > ARGS.max_file_size_encrypted_mb * 1024 * 1024)
                        || size > ARGS.max_file_size_unencrypted_mb * 1024 * 1024
                    {
                        return Err(actix_web::error::ErrorPayloadTooLarge("文件超过站点大小限制"));
                    }
                    f = web::block(move || f.write_all(&chunk).map(|_| f)).await??;
                }

                file.size = ByteSize::b(size as u64);
                staged_files.push((file, f));
            }
            "file_token" => {
                if ARGS.no_file_upload {
                    return Err(ErrorBadRequest("本站未启用文件上传"));
                }
                let mut token = Vec::new();
                while let Some(chunk) = field.try_next().await? {
                    if token.len() + chunk.len() > 32 {
                        return Err(ErrorBadRequest("分片编号无效"));
                    }
                    token.extend_from_slice(&chunk);
                }
                let token = String::from_utf8(token).map_err(|_| ErrorBadRequest("分片编号无效"))?;
                let chunks = chunks.clone();
                let staged = web::block(move || chunks.take(&token)).await?
                    .map_err(ErrorBadRequest)?;
                staged_files.push(staged);
            }
            field => {
                log::error!("Unexpected multipart field:  {}", field);
            }
        }
    }

    if ARGS.readonly && ARGS.uploader_password.is_some() {
        if uploader_password.trim() != ARGS.uploader_password.as_ref().unwrap().trim() {
            log::warn!("Uploader password mismatch. Input length: {}, Expected length: {}", uploader_password.trim().len(), ARGS.uploader_password.as_ref().unwrap().trim().len());
            return Ok(HttpResponse::Found()
                .append_header(("Location", format!("{}/incorrect", ARGS.public_path_as_str())))
                .finish());
        }
    }

    let mut names = std::collections::HashSet::new();
    if staged_files.iter().any(|(file, _)| !names.insert(file.name())) {
        return Err(ErrorBadRequest("有重名文件，请重命名后重新上传"));
    }

    let id = new_pasta.id;

    if plain_key != *"" && new_pasta.readonly {
        new_pasta.encrypted_key = Some(encrypt(id.to_string().as_str(), &plain_key));
    }

    if new_pasta.encrypt_server && !new_pasta.readonly && new_pasta.content != *"" {
        if new_pasta.encrypt_client {
            new_pasta.content = encrypt(&new_pasta.content, &random_key);
        } else {
            new_pasta.content = encrypt(&new_pasta.content, &plain_key);
        }
    }

    for (file, staged) in &staged_files {
        let limit = if new_pasta.encrypt_server {
            ARGS.max_file_size_encrypted_mb.min(ARGS.max_file_size_unencrypted_mb)
        } else {
            ARGS.max_file_size_unencrypted_mb
        };
        if file.size.as_u64() > limit as u64 * 1024 * 1024 {
            return Err(actix_web::error::ErrorPayloadTooLarge("文件超过站点大小限制"));
        }
        if new_pasta.encrypt_server && !new_pasta.readonly {
            let key = if new_pasta.encrypt_client { &random_key } else { &plain_key };
            encrypt_file(key, staged.path().to_str().unwrap())
                .map_err(actix_web::error::ErrorInternalServerError)?;
        }
    }

    let encrypt_server = new_pasta.encrypt_server;
    {
        let mut pastas = data.pastas.lock().unwrap();
        if pastas.iter().any(|p| p.id == id) {
            return Err(actix_web::error::ErrorConflict("分享编号碰巧重复，请重试"));
        }
        // Publish only complete files, while GC cannot mistake them for orphan uploads.
        let directory = format!("{}/attachments/{}", ARGS.data_dir, new_pasta.id_as_animals());
        if !staged_files.is_empty() {
            std::fs::create_dir_all(&directory)?;
        }
        for (file, staged) in staged_files {
            let suffix = if new_pasta.encrypt_server && !new_pasta.readonly { ".enc" } else { "" };
            staged.persist(format!("{}/{}{}", directory, file.name(), suffix))
                .map_err(actix_web::error::ErrorInternalServerError)?;
            if new_pasta.file.is_none() {
                new_pasta.file = Some(file);
            } else {
                new_pasta.attachments.get_or_insert_with(Vec::new).push(file);
            }
            new_pasta.pasta_type = String::from("text");
        }
        pastas.push(new_pasta);
        insert(Some(&pastas), pastas.last());
    }

    let slug = if ARGS.hash_ids {
        to_hashids(id)
    } else {
        to_animal_names(id)
    };

    if encrypt_server {
        Ok(HttpResponse::Found()
            .append_header(("Location", format!("{}/auth/{}/success", ARGS.public_path_as_str(), slug)))
            .finish())
    } else {
        // Generate time-limited token for initial view using Hashids
        let timenow = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let expiry = timenow + 15; // 15 seconds validity
        
        // Use global HARSH instance
        let encoded_token = crate::util::hashids::HARSH.encode(&[expiry, id]);

        Ok(HttpResponse::Found()
            .append_header((
                "Location",
                format!("{}/upload/{}", ARGS.public_path_as_str(), slug),
            ))
            .cookie(
                Cookie::build("owner_token", encoded_token)
                    .path("/")
                    .max_age(actix_web::cookie::time::Duration::seconds(15))
                    .finish(),
            )
            .finish())
    }
}
