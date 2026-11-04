use actix_multipart::Multipart;
use actix_web::{get, post, web, Error, HttpResponse};

use crate::args::ARGS;
use crate::endpoints::errors::ErrorTemplate;

use crate::util::auth;
use crate::util::db::delete;
use crate::util::misc::{decrypt, remove_expired};
use crate::AppState;
use crate::util::shortlinks::resolve_id;
use askama::Template;
use std::fs;

#[get("/remove/{id}")]
pub async fn remove(data: web::Data<AppState>, id: web::Path<String>) -> HttpResponse {
    let mut pastas = data.pastas.lock().unwrap();

    let id = resolve_id(&id, &pastas);

    for (i, pasta) in pastas.iter().enumerate() {
        if pasta.id == id {
            // if it's encrypted or read-only, it needs password to be deleted
            // OR if it is not editable (public immutable), it needs admin password to be deleted
            if pasta.encrypt_server || pasta.readonly || !pasta.editable {
                return HttpResponse::Found()
                    .append_header((
                        "Location",
                        format!("{}/auth_remove_private/{}", ARGS.public_path_as_str(), pasta.id_as_animals()),
                    ))
                    .finish();
            }

            // remove the directory and all its contents
            if fs::remove_dir_all(format!(
                "{}/attachments/{}/",
                ARGS.data_dir,
                pasta.id_as_animals()
            ))
            .is_err()
            {
                log::error!("Failed to delete directory for {}!", pasta.id_as_animals())
            }

            // remove it from in-memory pasta list
            pastas.remove(i);

            delete(Some(&pastas), Some(id));

            return HttpResponse::Found()
                .append_header(("Location", format!("{}/list", ARGS.public_path_as_str())))
                .finish();
        }
    }

    remove_expired(&mut pastas);

    HttpResponse::Ok()
        .content_type("text/html; charset=utf-8")
        .body(ErrorTemplate { args: &ARGS }.render().unwrap())
}

#[post("/remove/{id}")]
pub async fn post_remove(
    data: web::Data<AppState>,
    id: web::Path<String>,
    payload: Multipart,
) -> Result<HttpResponse, Error> {
    let mut pastas = data.pastas.lock().unwrap();

    let id = resolve_id(&id, &pastas);

    remove_expired(&mut pastas);

    let password = auth::password_from_multipart(payload).await?;

    for (i, pasta) in pastas.iter().enumerate() {
        if pasta.id == id {
            if pastas[i].readonly || pastas[i].encrypt_server || !pastas[i].editable {
                if password != *"" {
                    let mut is_password_correct = false;

                    if password == *ARGS.auth_admin_password {
                        is_password_correct = true;
                    }

                    // if it is read-only, the content is not encrypted, but the key is
                    if !is_password_correct && pastas[i].readonly {
                        if let Some(ref encrypted_key) = pastas[i].encrypted_key {
                            let res = decrypt(encrypted_key, &password);
                            if let Ok(decrypted_key) = res {
                                if decrypted_key == id.to_string() {
                                    is_password_correct = true;
                                }
                            }
                        }
                    } else if !is_password_correct && pastas[i].encrypt_server {
                        // if it is not read-only, the content is encrypted
                        let res = decrypt(pastas[i].content.to_owned().as_str(), &password);
                        if res.is_ok() {
                            is_password_correct = true;
                        }
                    }

                    if is_password_correct {
                // remove the directory and all its contents
                if fs::remove_dir_all(format!(
                    "{}/attachments/{}/",
                    ARGS.data_dir,
                    pasta.id_as_animals()
                ))
                .is_err()
                {
                    log::error!("Failed to delete directory for {}!", pasta.id_as_animals())
                }

                        // remove it from in-memory pasta list
                        pastas.remove(i);

                        delete(Some(&pastas), Some(id));

                        return Ok(HttpResponse::Found()
                            .append_header((
                                "Location",
                                format!("{}/list", ARGS.public_path_as_str()),
                            ))
                            .finish());
                    } else {
                        return Ok(HttpResponse::Found()
                            .append_header((
                                "Location",
                                format!("{}/auth_remove_private/{}/incorrect", ARGS.public_path_as_str(), pasta.id_as_animals()),
                            ))
                            .finish());
                    }
                } else {
                    return Ok(HttpResponse::Found()
                        .append_header((
                            "Location",
                            format!("{}/auth_remove_private/{}/incorrect", ARGS.public_path_as_str(), pasta.id_as_animals()),
                        ))
                        .finish());
                }
            }

            return Ok(HttpResponse::Found()
                .append_header((
                    "Location",
                    format!(
                        "{}/upload/{}",
                        ARGS.public_path_as_str(),
                        pastas[i].id_as_animals()
                    ),
                ))
                .finish());
        }
    }

    Ok(HttpResponse::Ok()
        .content_type("text/html; charset=utf-8")
        .body(ErrorTemplate { args: &ARGS }.render().unwrap()))
}

#[derive(serde::Deserialize)]
pub struct BatchRemove {
    slugs: Vec<String>,
}

pub async fn batch_remove(data: web::Data<AppState>, input: web::Json<BatchRemove>) -> Result<HttpResponse, Error> {
    if ARGS.no_listing { return Err(actix_web::error::ErrorForbidden("本站未启用列表管理")); }
    if input.slugs.is_empty() { return Err(actix_web::error::ErrorBadRequest("请先选择分享")); }
    let selected: std::collections::HashSet<_> = input.slugs.iter().collect();
    let mut pastas = data.pastas.lock().unwrap();
    let mut directories = Vec::new();
    // Validate the complete selection before changing any data.
    for slug in &selected {
        let pasta = pastas.iter().find(|p| p.id_as_animals() == slug.as_str())
            .ok_or_else(|| actix_web::error::ErrorConflict("部分分享已删除或过期，请刷新列表"))?;
        if pasta.private || !pasta.editable || pasta.readonly || pasta.encrypt_server {
            return Err(actix_web::error::ErrorForbidden("所选分享中有受保护内容，请通过单条管理验证密码"));
        }
        directories.push(format!("{}/attachments/{}", ARGS.data_dir, pasta.id_as_animals()));
    }
    pastas.retain(|p| !selected.contains(&p.id_as_animals()));
    crate::util::db::update_all(&pastas);
    for directory in directories {
        if let Err(error) = fs::remove_dir_all(&directory) {
            if error.kind() != std::io::ErrorKind::NotFound {
                log::error!("Failed to clean deleted share {directory}: {error}");
            }
        }
    }
    Ok(HttpResponse::Ok().insert_header(("Cache-Control", "no-store"))
        .json(serde_json::json!({"removed": selected.len()})))
}
