//! Common settings for the independent share and request-log connections.
use rusqlite::Connection;
use std::time::Duration;

pub fn configure(connection: &Connection) -> rusqlite::Result<()> {
    connection.busy_timeout(Duration::from_secs(5))?;
    connection.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA cache_size=-4096;
         PRAGMA wal_autocheckpoint=1000;
         PRAGMA journal_size_limit=16777216;",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reader_snapshot_does_not_block_writer_commit() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("wal.sqlite");
        let writer = Connection::open(&path).unwrap();
        configure(&writer).unwrap();
        writer.execute_batch("CREATE TABLE value (n INTEGER); INSERT INTO value VALUES (1);").unwrap();
        let reader = Connection::open(&path).unwrap();
        reader.execute_batch("BEGIN;").unwrap();
        let read = || reader.query_row("SELECT n FROM value", [], |row| row.get::<_, i64>(0)).unwrap();
        assert_eq!(read(), 1);
        writer.execute("UPDATE value SET n=2", []).unwrap();
        assert_eq!(read(), 1, "Readers keep their snapshot while writers commit");
        reader.execute_batch("COMMIT;").unwrap();
        assert_eq!(read(), 2);
    }
}
