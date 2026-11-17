//! HTTP request audit. Independent SQLite storage keeps retention work off the share database.
use actix_web::{
    body::MessageBody, dev::{ServiceRequest, ServiceResponse}, middleware::Next,
    web, Error, HttpMessage, HttpRequest,
};
use chrono::Utc;
use rusqlite::{params, Connection};
use std::{cell::RefCell, net::{IpAddr, SocketAddr}, path::Path, rc::Rc, sync::Mutex, time::{Duration, Instant}};

const RETENTION: i64 = 30 * 24 * 60 * 60;
const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS operation_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    created_at INTEGER NOT NULL,
    operation TEXT NOT NULL,
    method TEXT NOT NULL,
    path TEXT NOT NULL,
    status INTEGER NOT NULL,
    redirect_path TEXT,
    duration_ms INTEGER NOT NULL,
    client_ip TEXT,
    peer_ip TEXT,
    ip_source TEXT NOT NULL,
    forwarded_for TEXT,
    edgeone_ip TEXT,
    edgeone_request_id TEXT,
    share_slug TEXT,
    targets TEXT,
    expires_at INTEGER
);
CREATE INDEX IF NOT EXISTS operation_log_expiry ON operation_log(expires_at) WHERE expires_at IS NOT NULL;
CREATE INDEX IF NOT EXISTS operation_log_client ON operation_log(client_ip, created_at);
CREATE INDEX IF NOT EXISTS operation_log_share ON operation_log(share_slug, created_at);";

pub struct AuditLog {
    connection: Mutex<Connection>,
}

#[derive(Clone, Default)]
pub struct Targets(pub Rc<RefCell<Option<Vec<String>>>>);

struct Event {
    created_at: i64,
    operation: &'static str,
    method: String,
    path: String,
    status: u16,
    redirect_path: Option<String>,
    duration_ms: i64,
    client_ip: Option<String>,
    peer_ip: Option<String>,
    ip_source: &'static str,
    forwarded_for: Option<String>,
    edgeone_ip: Option<String>,
    edgeone_request_id: Option<String>,
    share_slug: Option<String>,
    targets: Option<String>,
    expires_at: Option<i64>,
}

impl AuditLog {
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        let connection = Connection::open(path)?;
        connection.busy_timeout(Duration::from_secs(5))?;
        // Set before creating tables so expired pages can be reclaimed without a full VACUUM.
        connection.execute_batch("PRAGMA auto_vacuum=INCREMENTAL; PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")?;
        connection.execute_batch(SCHEMA)?;
        let log = Self { connection: Mutex::new(connection) };
        log.cleanup(Utc::now().timestamp())?;
        Ok(log)
    }

    fn append(&self, event: &Event) -> rusqlite::Result<()> {
        self.connection.lock().unwrap().execute(
            "INSERT INTO operation_log (created_at, operation, method, path, status, duration_ms,
                client_ip, peer_ip, ip_source, forwarded_for, edgeone_ip, edgeone_request_id,
                share_slug, targets, expires_at, redirect_path)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
            params![event.created_at, event.operation, event.method, event.path, event.status,
                event.duration_ms, event.client_ip, event.peer_ip, event.ip_source,
                event.forwarded_for, event.edgeone_ip, event.edgeone_request_id,
                event.share_slug, event.targets, event.expires_at, event.redirect_path],
        )?;
        Ok(())
    }

    fn cleanup(&self, now: i64) -> rusqlite::Result<usize> {
        let connection = self.connection.lock().unwrap();
        let removed = connection.execute("DELETE FROM operation_log WHERE expires_at <= ?1", [now])?;
        connection.execute_batch("PRAGMA incremental_vacuum(256); PRAGMA wal_checkpoint(TRUNCATE);")?;
        Ok(removed)
    }

    pub fn start_cleanup(log: web::Data<Self>) {
        actix_web::rt::spawn(async move {
            loop {
                actix_web::rt::time::sleep(Duration::from_secs(60 * 60)).await;
                let log = log.clone();
                match web::block(move || log.cleanup(Utc::now().timestamp())).await {
                    Ok(Ok(removed)) => log::info!("Audit retention removed {removed} expired records"),
                    result => log::error!("Audit retention failed: {result:?}"),
                }
            }
        });
    }
}

fn parse_ip(value: &str) -> Option<String> {
    let value = value.trim();
    value.parse::<IpAddr>().ok().or_else(|| value.parse::<SocketAddr>().ok().map(|a| a.ip()))
        .map(|ip| ip.to_string())
}

fn header(request: &HttpRequest, name: &str, limit: usize) -> Option<String> {
    request.headers().get(name)?.to_str().ok().map(|s| s.chars().take(limit).collect())
}

fn client_ip(request: &HttpRequest) -> (Option<String>, &'static str) {
    // These are claims from the forwarding headers, not proof of identity. Keep the socket peer too.
    for (name, value) in [
        ("x-forwarded-for", request.headers().get("x-forwarded-for").and_then(|v| v.to_str().ok()).and_then(|s| s.split(',').next())),
        ("eo-connecting-ip", request.headers().get("eo-connecting-ip").and_then(|v| v.to_str().ok())),
        ("x-real-ip", request.headers().get("x-real-ip").and_then(|v| v.to_str().ok())),
    ] {
        if let Some(ip) = value.and_then(parse_ip) { return (Some(ip), name); }
    }
    (request.peer_addr().map(|addr| addr.ip().to_string()), "peer")
}

fn operation(method: &str, path: &str) -> (&'static str, bool) {
    let parts: Vec<_> = path.trim_matches('/').split('/').collect();
    match (method, parts.as_slice()) {
        ("POST", ["upload"]) => ("create", true),
        ("POST", ["edit" | "submit_edit_private", _]) => ("edit", true),
        ("GET" | "POST", ["remove", _]) => ("delete", true),
        ("POST", ["list", "remove"]) => ("batch_delete", true),
        ("POST", ["admin"] | ["edit_private", _]) => ("authenticate", true),
        ("POST", ["upload", "chunks", "start"]) => ("upload_start", false),
        ("PUT", ["upload", "chunks", _, _]) => ("upload_chunk", false),
        ("DELETE", ["upload", "chunks", _]) => ("upload_cancel", false),
        ("POST", ["upload", "options", "availability"]) => ("format_check", false),
        _ => ("access", false),
    }
}

fn internal_redirect(location: &str) -> Option<String> {
    let base = crate::args::ARGS.public_path_as_str();
    let path = if location.starts_with('/') { location } else {
        if base.is_empty() { return None; }
        location.strip_prefix(&base)?
    };
    if !path.starts_with('/') || path.starts_with("//") { return None; }
    Some(path.split(['?', '#']).next()?.to_owned())
}

fn created_slug(location: &str) -> Option<String> {
    let base = crate::args::ARGS.public_path_as_str();
    let parts: Vec<_> = location.strip_prefix(&base)?.trim_matches('/').split('/').collect();
    match parts.as_slice() {
        ["upload", slug] | ["auth", slug, "success"] => Some(slug.to_string()),
        _ => None,
    }
}

/// Record errors and rejected requests too, before returning response headers to the client.
/// For streamed files this is a request record, not proof that the complete download reached the client.
pub async fn record<B: MessageBody + 'static>(request: ServiceRequest, next: Next<B>) -> Result<ServiceResponse<B>, Error> {
    let log = request.app_data::<web::Data<AuditLog>>().unwrap().clone();
    let targets = Targets::default();
    request.extensions_mut().insert(targets.clone());
    let started = Instant::now();
    let created_at = Utc::now().timestamp();
    let method = request.method().as_str().to_owned();
    let path = request.path().to_owned();
    let (operation, permanent) = operation(&method, &path);
    let (client_ip, ip_source) = client_ip(request.request());
    let peer_ip = request.peer_addr().map(|addr| addr.ip().to_string());
    let forwarded_for = header(request.request(), "x-forwarded-for", 2048);
    let edgeone_ip = header(request.request(), "eo-connecting-ip", 128);
    let edgeone_request_id = header(request.request(), "eo-log-uuid", 128);
    let response = next.call(request).await;
    let status = match &response {
        Ok(response) => response.status().as_u16(),
        Err(error) => error.as_response_error().status_code().as_u16(),
    };
    let redirect_path = response.as_ref().ok().and_then(|response| response.headers().get("location"))
        .and_then(|value| value.to_str().ok()).and_then(internal_redirect);
    let mut share_slug = if path.starts_with("/upload/chunks/") { None } else {
        response.as_ref().ok().and_then(|response| response.request().match_info().get("id")).map(str::to_owned)
    };
    if operation == "create" {
        share_slug = response.as_ref().ok().and_then(|response| response.headers().get("location"))
            .and_then(|value| value.to_str().ok()).and_then(created_slug);
    }
    let targets = targets.0.borrow().as_ref().map(|targets| serde_json::to_string(targets).unwrap());
    let event = Event {
        created_at, operation, method, path, status, redirect_path, duration_ms: started.elapsed().as_millis() as i64,
        client_ip, peer_ip, ip_source, forwarded_for, edgeone_ip, edgeone_request_id, share_slug, targets,
        expires_at: if permanent { None } else { Some(created_at + RETENTION) },
    };
    // No in-memory queue: the committed row survives an immediate process/container restart.
    match web::block(move || log.append(&event)).await {
        Ok(Ok(())) => (),
        result => log::error!("Could not persist request audit: {result:?}"),
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::{test::TestRequest, App, HttpResponse};

    #[test]
    fn forwarding_headers_do_not_replace_the_socket_peer() {
        let req = TestRequest::default().peer_addr("192.0.2.10:54321".parse().unwrap())
            .insert_header(("x-forwarded-for", "2001:db8::1, 198.51.100.7"))
            .insert_header(("eo-connecting-ip", "198.51.100.7")).to_http_request();
        assert_eq!(client_ip(&req), (Some("2001:db8::1".into()), "x-forwarded-for"));
        assert_eq!(req.peer_addr().unwrap().ip().to_string(), "192.0.2.10");
        let req = TestRequest::default().insert_header(("x-forwarded-for", "unknown, 1.2.3.4"))
            .insert_header(("eo-connecting-ip", "198.51.100.7")).to_http_request();
        assert_eq!(client_ip(&req), (Some("198.51.100.7".into()), "eo-connecting-ip"));
        for (input, expected) in [("1.2.3.4:80", "1.2.3.4"), ("[2001:db8::2]:443", "2001:db8::2"), ("::1", "::1")] {
            assert_eq!(parse_ip(input).as_deref(), Some(expected));
        }
        assert!(parse_ip("unknown").is_none());
    }

    #[actix_web::test]
    async fn errors_auth_and_retention_are_persisted() {
        let directory = tempfile::tempdir().unwrap();
        let log = web::Data::new(AuditLog::open(&directory.path().join("audit.sqlite")).unwrap());
        let app = actix_web::test::init_service(App::new().app_data(log.clone())
            .route("/fail", web::get().to(|| async { Err::<HttpResponse, _>(actix_web::error::ErrorInternalServerError("test")) }))
            .route("/remove/abcde", web::get().to(|| async { HttpResponse::Unauthorized().finish() }))
            .wrap(actix_web::middleware::from_fn(record))).await;
        for (path, expected) in [("/fail", 500), ("/remove/abcde", 401), ("/missing", 404)] {
            let req = TestRequest::get().uri(path).peer_addr("127.0.0.1:22".parse().unwrap()).to_request();
            let response = actix_web::test::call_service(&app, req).await;
            assert_eq!(response.status().as_u16(), expected);
        }
        {
            let conn = log.connection.lock().unwrap();
            assert_eq!(conn.query_row("SELECT count(*) FROM operation_log", [], |r| r.get::<_, i64>(0)).unwrap(), 3);
            conn.execute("UPDATE operation_log SET created_at=1, expires_at=2 WHERE operation='access'", []).unwrap();
            conn.execute("UPDATE operation_log SET created_at=1 WHERE operation='delete'", []).unwrap();
        }
        assert_eq!(log.cleanup(2).unwrap(), 2);
        let conn = log.connection.lock().unwrap();
        assert_eq!(conn.query_row("SELECT operation FROM operation_log", [], |r| r.get::<_, String>(0)).unwrap(), "delete");
    }
}
