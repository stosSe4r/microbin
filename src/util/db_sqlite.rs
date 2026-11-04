use bytesize::ByteSize;
use rusqlite::{params, Connection};

use crate::{args::ARGS, pasta::PastaFile, Pasta};

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS pasta (
    id INTEGER PRIMARY KEY,
    content TEXT NOT NULL,
    file_name TEXT,
    file_size INTEGER,
    extension TEXT NOT NULL,
    read_only INTEGER NOT NULL,
    private INTEGER NOT NULL,
    editable INTEGER NOT NULL,
    encrypt_server INTEGER NOT NULL,
    encrypt_client INTEGER NOT NULL,
    encrypted_key TEXT,
    created INTEGER NOT NULL,
    expiration INTEGER NOT NULL,
    last_read INTEGER NOT NULL,
    read_count INTEGER NOT NULL,
    burn_after_reads INTEGER NOT NULL,
    attachments TEXT,
    slug TEXT,
    pasta_type TEXT NOT NULL
);";

fn connect() -> Connection {
    let conn = Connection::open(format!("{}/database.sqlite", ARGS.data_dir))
        .expect("Failed to open SQLite database!");
    conn.execute_batch(SCHEMA)
        .expect("Failed to create SQLite table for Pasta!");
    let has_slug = conn.prepare("PRAGMA table_info(pasta)").unwrap()
        .query_map([], |row| row.get::<_, String>(1)).unwrap()
        .any(|name| name.unwrap() == "slug");
    if !has_slug {
        conn.execute_batch("ALTER TABLE pasta ADD COLUMN slug TEXT;")
            .expect("Failed to add short-link column");
    }
    conn.execute_batch("CREATE UNIQUE INDEX IF NOT EXISTS pasta_slug ON pasta(slug);")
        .expect("Failed to index short links");
    conn
}

pub fn read_all() -> Vec<Pasta> {
    select_all(&connect()).expect("Failed to load pastas from SQLite database")
}

pub fn update_all(pastas: &[Pasta]) {
    rewrite_all(&mut connect(), pastas).expect("Failed to save pastas to SQLite database");
}

fn rewrite_all(conn: &mut Connection, pastas: &[Pasta]) -> rusqlite::Result<()> {
    // Keep the previous snapshot intact if any insert fails or the process stops.
    let transaction = conn.transaction()?;
    transaction.execute("DELETE FROM pasta", [])?;
    for pasta in pastas {
        insert_into(&transaction, pasta)?;
    }
    transaction.commit()
}

fn select_all(conn: &Connection) -> rusqlite::Result<Vec<Pasta>> {
    let mut stmt = conn.prepare("SELECT * FROM pasta ORDER BY created ASC")?;

    let pasta_iter = stmt.query_map([], |row| {
        Ok(Pasta {
            id: row.get("id")?,
            slug: row.get("slug")?,
            content: row.get("content")?,
            file: if let (Some(file_name), Some(file_size)) = (
                row.get::<_, Option<String>>("file_name")?,
                row.get("file_size")?,
            ) {
                let file_size: u64 = file_size;
                if !file_name.is_empty() {
                    Some(PastaFile {
                        name: file_name,
                        size: ByteSize::b(file_size),
                    })
                } else {
                    None
                }
            } else {
                None
            },
            extension: row.get("extension")?,
            readonly: row.get("read_only")?,
            private: row.get("private")?,
            editable: row.get("editable")?,
            encrypt_server: row.get("encrypt_server")?,
            encrypt_client: row.get("encrypt_client")?,
            encrypted_key: row.get("encrypted_key")?,
            created: row.get("created")?,
            expiration: row.get("expiration")?,
            last_read: row.get("last_read")?,
            read_count: row.get("read_count")?,
            burn_after_reads: row.get("burn_after_reads")?,
            pasta_type: row.get("pasta_type")?,
            attachments: match row.get::<_, Option<String>>("attachments")? {
                Some(json) => serde_json::from_str(&json).map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        row.as_ref().column_index("attachments").unwrap(),
                        rusqlite::types::Type::Text,
                        Box::new(error),
                    )
                })?,
                None => None,
            },
        })
    })?;

    pasta_iter.collect()
}

pub fn insert(pasta: &Pasta) {
    insert_into(&connect(), pasta).expect("Failed to insert pasta");
}

fn insert_into(conn: &Connection, pasta: &Pasta) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO pasta (
                id,
                content,
                file_name,
                file_size,
                extension,
                read_only,
                private,
                editable,
                encrypt_server,
                encrypt_client,
                encrypted_key,
                created,
                expiration,
                last_read,
                read_count,
                burn_after_reads,
                pasta_type,
                attachments,
                slug
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)",
        params![
            pasta.id,
            pasta.content,
            pasta.file.as_ref().map_or("", |f| f.name.as_str()),
            pasta.file.as_ref().map_or(0, |f| f.size.as_u64()),
            pasta.extension,
            pasta.readonly as i32,
            pasta.private as i32,
            pasta.editable as i32,
            pasta.encrypt_server as i32,
            pasta.encrypt_client as i32,
            pasta.encrypted_key.as_deref(),
            pasta.created,
            pasta.expiration,
            pasta.last_read,
            pasta.read_count,
            pasta.burn_after_reads,
            pasta.pasta_type,
            serde_json::to_string(&pasta.attachments).expect("Failed to serialize attachments"),
            pasta.slug,
        ],
    )?;
    Ok(())
}

pub fn update(pasta: &Pasta) {
    let conn = connect();

    conn.execute(
        "UPDATE pasta SET
            content = ?2,
            file_name = ?3,
            file_size = ?4,
            extension = ?5,
            read_only = ?6,
            private = ?7,
            editable = ?8,
            encrypt_server = ?9,
            encrypt_client = ?10,
            encrypted_key = ?11,
            created = ?12,
            expiration = ?13,
            last_read = ?14,
            read_count = ?15,
            burn_after_reads = ?16,
            pasta_type = ?17,
            attachments = ?18,
            slug = ?19
        WHERE id = ?1;",
        params![
            pasta.id,
            pasta.content,
            pasta.file.as_ref().map_or("", |f| f.name.as_str()),
            pasta.file.as_ref().map_or(0, |f| f.size.as_u64()),
            pasta.extension,
            pasta.readonly as i32,
            pasta.private as i32,
            pasta.editable as i32,
            pasta.encrypt_server as i32,
            pasta.encrypt_client as i32,
            pasta.encrypted_key.as_deref(),
            pasta.created,
            pasta.expiration,
            pasta.last_read,
            pasta.read_count,
            pasta.burn_after_reads,
            pasta.pasta_type,
            serde_json::to_string(&pasta.attachments).expect("Failed to serialize attachments"),
            pasta.slug,
        ],
    )
    .expect("Failed to update pasta.");
}

pub fn delete_by_id(id: u64) {
    let conn = connect();

    conn.execute(
        "DELETE FROM pasta 
        WHERE id = ?1;",
        params![id],
    )
    .expect("Failed to delete pasta.");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sqlite_reopens_fields_and_rolls_back_failed_snapshot() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let mut conn = Connection::open(file.path()).unwrap();
        conn.execute_batch(SCHEMA).unwrap();
        let mut pasta = Pasta {
            id: 42,
            slug: Some("abcde".into()),
            content: "https://example.org/中文".into(),
            file: Some(PastaFile {
                name: "empty.bin".into(),
                size: ByteSize::b(0),
            }),
            attachments: Some(vec![PastaFile {
                name: "附件.png".into(),
                size: ByteSize::b(68),
            }]),
            extension: "txt".into(),
            readonly: true,
            private: false,
            editable: true,
            encrypt_server: false,
            encrypt_client: true,
            encrypted_key: Some("key".into()),
            created: 1700000000,
            expiration: 1800000000,
            last_read: 1700000020,
            read_count: 3,
            burn_after_reads: 10,
            pasta_type: "url".into(),
        };
        insert_into(&conn, &pasta).unwrap();
        let expected = serde_json::to_value(&pasta).unwrap();
        drop(conn);
        conn = Connection::open(file.path()).unwrap();
        assert_eq!(
            serde_json::to_value(&select_all(&conn).unwrap()[0]).unwrap(),
            expected
        );

        // A duplicate ID must roll back both DELETE and the earlier successful INSERT.
        pasta.id = 43;
        let duplicate =
            serde_json::from_value(serde_json::to_value(&pasta).unwrap()).unwrap();
        assert!(rewrite_all(&mut conn, &[pasta, duplicate]).is_err());
        assert_eq!(
            serde_json::to_value(&select_all(&conn).unwrap()[0]).unwrap(),
            expected
        );
        assert_eq!(select_all(&conn).unwrap().len(), 1);
        let replacement = serde_json::from_value(expected.clone()).unwrap();
        rewrite_all(&mut conn, &[replacement]).unwrap();
        assert_eq!(
            serde_json::to_value(&select_all(&conn).unwrap()[0]).unwrap(),
            expected
        );

        conn.execute("UPDATE pasta SET attachments = 'broken JSON'", [])
            .unwrap();
        assert!(
            select_all(&conn).is_err(),
            "Corrupt attachments must not be silently dropped"
        );
    }
}
