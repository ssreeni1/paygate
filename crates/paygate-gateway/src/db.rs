use paygate_common::types::{BaseUnits, PaymentRecord, Quote};
use rusqlite::{Connection, params};
use thiserror::Error;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

#[derive(Debug, Error)]
pub enum DbError {
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("writer channel closed")]
    ChannelClosed,
    #[error("writer channel full — backpressure (503)")]
    Backpressure,
}

/// Active session info for CLI display.
#[derive(Debug, Clone)]
pub struct SessionRecord {
    pub id: String,
    pub payer_address: String,
    pub balance: u64,
    pub rate_per_request: u64,
    pub requests_made: u64,
    pub expires_at: i64,
}

/// Commands sent to the single-writer task via bounded mpsc channel.
#[derive(Debug)]
pub enum WriteCommand {
    InsertPayment {
        record: PaymentRecord,
        reply: tokio::sync::oneshot::Sender<Result<(), DbError>>,
    },
    InsertQuote {
        quote: Quote,
    },
    ConsumeQuote {
        id: String,
    },
    InsertRequestLog {
        payment_id: Option<String>,
        session_id: Option<String>,
        endpoint: String,
        payer_address: String,
        amount_charged: BaseUnits,
        upstream_status: Option<i32>,
        upstream_latency_ms: Option<i64>,
    },
}

/// Read-only database handle for queries. Multiple readers are safe with WAL mode.
#[derive(Clone)]
pub struct DbReader {
    path: String,
}

impl DbReader {
    pub fn new(path: &str) -> Self {
        Self {
            path: path.to_string(),
        }
    }

    fn conn(&self) -> Result<Connection, DbError> {
        let conn = Connection::open(&self.path)?;
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA busy_timeout = 5000;")?;
        Ok(conn)
    }

    /// Check if a tx_hash has already been consumed (replay protection).
    pub fn is_tx_consumed(&self, tx_hash: &str) -> Result<bool, DbError> {
        let conn = self.conn()?;
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM payments WHERE tx_hash = ?",
            params![tx_hash],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    /// Look up a quote by ID. Returns None if not found or expired.
    pub fn get_quote(&self, id: &str) -> Result<Option<Quote>, DbError> {
        let conn = self.conn()?;
        let now = chrono::Utc::now().timestamp();
        let mut stmt = conn.prepare(
            "SELECT id, endpoint, price, token_address, created_at, expires_at
             FROM quotes WHERE id = ? AND expires_at > ?",
        )?;
        let result = stmt.query_row(params![id, now], |row| {
            Ok(Quote {
                id: row.get(0)?,
                endpoint: row.get(1)?,
                price: row.get::<_, i64>(2)? as BaseUnits,
                token: row.get::<_, String>(3)?.parse().unwrap_or_default(),
                created_at: row.get(4)?,
                expires_at: row.get(5)?,
            })
        });
        match result {
            Ok(q) => Ok(Some(q)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(DbError::Sqlite(e)),
        }
    }

    /// Look up a payment by tx_hash.
    pub fn get_payment(&self, tx_hash: &str) -> Result<Option<PaymentRecord>, DbError> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT id, tx_hash, payer_address, amount, token_address, endpoint,
                    request_hash, quote_id, block_number, verified_at, status
             FROM payments WHERE tx_hash = ?",
        )?;
        let result = stmt.query_row(params![tx_hash], |row| {
            Ok(PaymentRecord {
                id: row.get(0)?,
                tx_hash: row.get(1)?,
                payer_address: row.get(2)?,
                amount: row.get::<_, i64>(3)? as BaseUnits,
                token_address: row.get(4)?,
                endpoint: row.get(5)?,
                request_hash: row.get(6)?,
                quote_id: row.get(7)?,
                block_number: row.get::<_, i64>(8)? as u64,
                verified_at: row.get(9)?,
                status: row.get(10)?,
            })
        });
        match result {
            Ok(p) => Ok(Some(p)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(DbError::Sqlite(e)),
        }
    }

    /// Get revenue summary (total amount, request count) for a time window.
    pub fn revenue_summary(&self, since_timestamp: i64) -> Result<(BaseUnits, u64), DbError> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT COALESCE(SUM(amount_charged), 0), COUNT(*)
             FROM request_log WHERE created_at >= ?",
        )?;
        let (total, count) = stmt.query_row(params![since_timestamp], |row| {
            Ok((row.get::<_, i64>(0)? as BaseUnits, row.get::<_, i64>(1)? as u64))
        })?;
        Ok((total, count))
    }

    /// Get revenue by endpoint for a time window.
    pub fn revenue_by_endpoint(
        &self,
        since_timestamp: i64,
    ) -> Result<Vec<(String, BaseUnits, u64)>, DbError> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT endpoint, COALESCE(SUM(amount_charged), 0), COUNT(*)
             FROM request_log WHERE created_at >= ?
             GROUP BY endpoint ORDER BY SUM(amount_charged) DESC",
        )?;
        let rows = stmt.query_map(params![since_timestamp], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)? as BaseUnits,
                row.get::<_, i64>(2)? as u64,
            ))
        })?;
        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }

    /// Count active sessions.
    pub fn active_session_count(&self) -> Result<u64, DbError> {
        let conn = self.conn()?;
        let now = chrono::Utc::now().timestamp();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM sessions WHERE status = 'active' AND expires_at > ?",
            params![now],
            |row| row.get(0),
        )?;
        Ok(count as u64)
    }

    /// List active sessions.
    pub fn list_active_sessions(&self) -> Result<Vec<SessionRecord>, DbError> {
        let conn = self.conn()?;
        let now = chrono::Utc::now().timestamp();
        let mut stmt = conn.prepare(
            "SELECT id, payer_address, balance, rate_per_request, requests_made, expires_at
             FROM sessions WHERE status = 'active' AND expires_at > ?
             ORDER BY expires_at ASC",
        )?;
        let rows = stmt.query_map(params![now], |row| {
            Ok(SessionRecord {
                id: row.get(0)?,
                payer_address: row.get(1)?,
                balance: row.get::<_, i64>(2)? as u64,
                rate_per_request: row.get::<_, i64>(3)? as u64,
                requests_made: row.get::<_, i64>(4)? as u64,
                expires_at: row.get(5)?,
            })
        })?;
        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }

    /// Get recent transactions ordered by verified_at descending.
    pub fn recent_transactions(
        &self,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<PaymentRecord>, DbError> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT id, tx_hash, payer_address, amount, token_address, endpoint,
                    request_hash, quote_id, block_number, verified_at, status
             FROM payments ORDER BY verified_at DESC LIMIT ? OFFSET ?",
        )?;
        let rows = stmt.query_map(params![limit, offset], |row| {
            Ok(PaymentRecord {
                id: row.get(0)?,
                tx_hash: row.get(1)?,
                payer_address: row.get(2)?,
                amount: row.get::<_, i64>(3)? as BaseUnits,
                token_address: row.get(4)?,
                endpoint: row.get(5)?,
                request_hash: row.get(6)?,
                quote_id: row.get(7)?,
                block_number: row.get::<_, i64>(8)? as u64,
                verified_at: row.get(9)?,
                status: row.get(10)?,
            })
        })?;
        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }

    /// Get total transaction count and total revenue.
    pub fn transaction_stats(&self) -> Result<(u64, BaseUnits), DbError> {
        let conn = self.conn()?;
        let (count, revenue) = conn.query_row(
            "SELECT COUNT(*), COALESCE(SUM(amount), 0) FROM payments",
            [],
            |row| {
                Ok((
                    row.get::<_, i64>(0)? as u64,
                    row.get::<_, i64>(1)? as BaseUnits,
                ))
            },
        )?;
        Ok((count, revenue))
    }

    /// Count active quotes (for metrics).
    pub fn active_quote_count(&self) -> Result<u64, DbError> {
        let conn = self.conn()?;
        let now = chrono::Utc::now().timestamp();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM quotes WHERE expires_at > ?",
            params![now],
            |row| row.get(0),
        )?;
        Ok(count as u64)
    }
}

/// Writer task handle. Send commands through this.
#[derive(Clone)]
pub struct DbWriter {
    tx: mpsc::Sender<WriteCommand>,
}

impl DbWriter {
    /// Send a payment insert and wait for confirmation. Returns error on replay (UNIQUE violation).
    pub async fn insert_payment(&self, record: PaymentRecord) -> Result<(), DbError> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .try_send(WriteCommand::InsertPayment {
                record,
                reply: reply_tx,
            })
            .map_err(|e| match e {
                mpsc::error::TrySendError::Full(_) => DbError::Backpressure,
                mpsc::error::TrySendError::Closed(_) => DbError::ChannelClosed,
            })?;
        reply_rx.await.map_err(|_| DbError::ChannelClosed)?
    }

    /// Insert a quote (fire-and-forget).
    pub async fn insert_quote(&self, quote: Quote) -> Result<(), DbError> {
        self.tx
            .try_send(WriteCommand::InsertQuote { quote })
            .map_err(|e| match e {
                mpsc::error::TrySendError::Full(_) => DbError::Backpressure,
                mpsc::error::TrySendError::Closed(_) => DbError::ChannelClosed,
            })?;
        Ok(())
    }

    /// Consume a quote after successful verification (fire-and-forget).
    pub async fn consume_quote(&self, id: String) -> Result<(), DbError> {
        self.tx
            .try_send(WriteCommand::ConsumeQuote { id })
            .map_err(|e| match e {
                mpsc::error::TrySendError::Full(_) => DbError::Backpressure,
                mpsc::error::TrySendError::Closed(_) => DbError::ChannelClosed,
            })?;
        Ok(())
    }

    /// Log a request (fire-and-forget).
    pub async fn log_request(
        &self,
        payment_id: Option<String>,
        session_id: Option<String>,
        endpoint: String,
        payer_address: String,
        amount_charged: BaseUnits,
        upstream_status: Option<i32>,
        upstream_latency_ms: Option<i64>,
    ) -> Result<(), DbError> {
        self.tx
            .try_send(WriteCommand::InsertRequestLog {
                payment_id,
                session_id,
                endpoint,
                payer_address,
                amount_charged,
                upstream_status,
                upstream_latency_ms,
            })
            .map_err(|e| match e {
                mpsc::error::TrySendError::Full(_) => DbError::Backpressure,
                mpsc::error::TrySendError::Closed(_) => DbError::ChannelClosed,
            })?;
        Ok(())
    }

    /// Current channel queue depth (for metrics).
    pub fn queue_depth(&self) -> usize {
        // mpsc::Sender doesn't expose queue depth directly;
        // we track it via the capacity - available permits pattern.
        // For now, return 0 — will be refined during implementation.
        0
    }
}

const CHANNEL_CAPACITY: usize = 10_000;

/// Initialize the database and spawn the writer task. Returns (reader, writer).
pub fn init_db(path: &str) -> Result<(DbReader, DbWriter), DbError> {
    // Create schema
    let conn = Connection::open(path)?;
    conn.execute_batch(include_str!("../../../schema.sql"))?;
    drop(conn);

    let reader = DbReader::new(path);
    let (tx, rx) = mpsc::channel(CHANNEL_CAPACITY);
    let writer = DbWriter { tx };

    // Spawn writer task
    let db_path = path.to_string();
    tokio::spawn(async move {
        writer_task(db_path, rx).await;
    });

    Ok((reader, writer))
}

async fn writer_task(path: String, mut rx: mpsc::Receiver<WriteCommand>) {
    let conn = match Connection::open(&path) {
        Ok(c) => c,
        Err(e) => {
            error!("Failed to open DB for writer task: {e}");
            return;
        }
    };
    if let Err(e) = conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA busy_timeout = 5000;") {
        error!("Failed to set WAL mode: {e}");
        return;
    }

    // Batch writes: flush every 10ms or 50 writes
    let mut batch = Vec::with_capacity(50);
    let flush_interval = tokio::time::Duration::from_millis(10);

    loop {
        // Collect up to 50 commands or timeout
        let deadline = tokio::time::Instant::now() + flush_interval;
        loop {
            let timeout = tokio::time::timeout_at(deadline, rx.recv());
            match timeout.await {
                Ok(Some(cmd)) => {
                    batch.push(cmd);
                    if batch.len() >= 50 {
                        break;
                    }
                }
                Ok(None) => {
                    // Channel closed — flush remaining and exit
                    flush_batch(&conn, &mut batch);
                    info!("DB writer task shutting down");
                    return;
                }
                Err(_) => break, // Timeout — flush what we have
            }
        }

        if !batch.is_empty() {
            flush_batch(&conn, &mut batch);
        }
    }
}

fn flush_batch(conn: &Connection, batch: &mut Vec<WriteCommand>) {
    if batch.is_empty() {
        return;
    }

    let tx_result = conn.execute_batch("BEGIN");
    if let Err(e) = tx_result {
        error!("Failed to begin transaction: {e}");
        // Drop all commands — callers will get ChannelClosed
        batch.clear();
        return;
    }

    for cmd in batch.drain(..) {
        match cmd {
            WriteCommand::InsertPayment { record, reply } => {
                let result = conn.execute(
                    "INSERT INTO payments (id, tx_hash, payer_address, amount, token_address,
                     endpoint, request_hash, quote_id, block_number, verified_at, status)
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                    params![
                        record.id,
                        record.tx_hash,
                        record.payer_address,
                        record.amount as i64,
                        record.token_address,
                        record.endpoint,
                        record.request_hash,
                        record.quote_id,
                        record.block_number as i64,
                        record.verified_at,
                        record.status,
                    ],
                );
                let _ = reply.send(result.map(|_| ()).map_err(DbError::Sqlite));
            }
            WriteCommand::InsertQuote { quote } => {
                let _ = conn.execute(
                    "INSERT INTO quotes (id, endpoint, price, token_address, created_at, expires_at)
                     VALUES (?, ?, ?, ?, ?, ?)",
                    params![
                        quote.id,
                        quote.endpoint,
                        quote.price as i64,
                        quote.token.to_string(),
                        quote.created_at,
                        quote.expires_at,
                    ],
                );
            }
            WriteCommand::ConsumeQuote { id } => {
                let _ = conn.execute("DELETE FROM quotes WHERE id = ?", params![id]);
            }
            WriteCommand::InsertRequestLog {
                payment_id,
                session_id,
                endpoint,
                payer_address,
                amount_charged,
                upstream_status,
                upstream_latency_ms,
            } => {
                let now = chrono::Utc::now().timestamp();
                let _ = conn.execute(
                    "INSERT INTO request_log (payment_id, session_id, endpoint, payer_address,
                     amount_charged, upstream_status, upstream_latency_ms, created_at)
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                    params![
                        payment_id,
                        session_id,
                        endpoint,
                        payer_address,
                        amount_charged as i64,
                        upstream_status,
                        upstream_latency_ms,
                        now,
                    ],
                );
            }
        }
    }

    if let Err(e) = conn.execute_batch("COMMIT") {
        error!("Failed to commit batch: {e}");
    }
}

/// Run periodic cleanup (call from a spawned task).
pub async fn cleanup_task(reader: DbReader, retention_days: u32) {
    let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(300)); // 5 min

    loop {
        interval.tick().await;

        // Clean up expired quotes
        if let Ok(conn) = reader.conn() {
            let now = chrono::Utc::now().timestamp();
            let cutoff = now - 3600; // quotes older than 1 hour past expiry
            match conn.execute("DELETE FROM quotes WHERE expires_at < ?", params![cutoff]) {
                Ok(n) if n > 0 => info!("Cleaned up {n} expired quotes"),
                Err(e) => warn!("Quote cleanup failed: {e}"),
                _ => {}
            }

            // Update metrics gauges
            if let Ok(count) = reader.active_quote_count() {
                crate::metrics::set_active_quotes(count);
            }
            // TODO: set_active_sessions when sessions are implemented (Wave 2)

            // Clean up old request logs (batched to avoid blocking)
            let log_cutoff = now - (retention_days as i64 * 86400);
            loop {
                let deleted = conn.execute(
                    "DELETE FROM request_log WHERE rowid IN
                     (SELECT rowid FROM request_log WHERE created_at < ? LIMIT 5000)",
                    params![log_cutoff],
                );
                match deleted {
                    Ok(n) if n > 0 => {
                        info!("Cleaned up {n} old request log entries");
                        if n < 5000 {
                            break;
                        }
                        // Yield between batches
                        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
                    }
                    Ok(_) => break,
                    Err(e) => {
                        warn!("Request log cleanup failed: {e}");
                        break;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn setup_test_db() -> (String, DbReader) {
        let path = format!("/tmp/paygate_db_test_{}.db", uuid::Uuid::new_v4());
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(include_str!("../../../schema.sql")).unwrap();
        drop(conn);
        let reader = DbReader::new(&path);
        (path, reader)
    }

    fn insert_payment(path: &str, id: &str, tx_hash: &str, amount: i64, verified_at: i64) {
        let conn = Connection::open(path).unwrap();
        conn.execute(
            "INSERT INTO payments (id, tx_hash, payer_address, amount, token_address, endpoint,
                                   block_number, verified_at, status)
             VALUES (?, ?, '0x9E2b000000000000000000000000000000000001', ?, '0x20c0000000000000000000000000000000000000',
                     'POST /v1/echo', 100, ?, 'verified')",
            params![id, tx_hash, amount, verified_at],
        ).unwrap();
    }

    // T1: Recent transactions ordered by verified_at DESC
    #[test]
    fn test_recent_transactions_ordered() {
        let (path, reader) = setup_test_db();
        insert_payment(&path, "id1", "0xaaa1", 1000, 1000);
        insert_payment(&path, "id2", "0xaaa2", 2000, 3000);
        insert_payment(&path, "id3", "0xaaa3", 3000, 2000);

        let txs = reader.recent_transactions(10, 0).unwrap();
        assert_eq!(txs.len(), 3);
        assert_eq!(txs[0].verified_at, 3000); // most recent first
        assert_eq!(txs[1].verified_at, 2000);
        assert_eq!(txs[2].verified_at, 1000);

        let _ = std::fs::remove_file(&path);
    }

    // T2: Recent transactions on empty DB returns empty vec
    #[test]
    fn test_recent_transactions_empty_db() {
        let (path, reader) = setup_test_db();
        let txs = reader.recent_transactions(10, 0).unwrap();
        assert!(txs.is_empty());
        let _ = std::fs::remove_file(&path);
    }

    // T3: Transaction stats returns correct count and revenue
    #[test]
    fn test_transaction_stats_correct() {
        let (path, reader) = setup_test_db();
        insert_payment(&path, "id1", "0xbbb1", 1000, 100);
        insert_payment(&path, "id2", "0xbbb2", 2000, 200);
        insert_payment(&path, "id3", "0xbbb3", 3000, 300);

        let (count, revenue) = reader.transaction_stats().unwrap();
        assert_eq!(count, 3);
        assert_eq!(revenue, 6000);

        let _ = std::fs::remove_file(&path);
    }
}
