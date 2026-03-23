PRAGMA journal_mode = WAL;

CREATE TABLE IF NOT EXISTS payments (
    id              TEXT PRIMARY KEY,
    tx_hash         TEXT UNIQUE NOT NULL,
    payer_address   TEXT NOT NULL,
    amount          INTEGER NOT NULL,
    token_address   TEXT NOT NULL,
    endpoint        TEXT NOT NULL,
    request_hash    TEXT,
    quote_id        TEXT,
    block_number    INTEGER NOT NULL,
    verified_at     INTEGER NOT NULL,
    status          TEXT NOT NULL DEFAULT 'verified'
);
CREATE INDEX IF NOT EXISTS idx_payments_payer ON payments(payer_address);
CREATE INDEX IF NOT EXISTS idx_payments_verified ON payments(verified_at);

CREATE TABLE IF NOT EXISTS quotes (
    id              TEXT PRIMARY KEY,
    endpoint        TEXT NOT NULL,
    price           INTEGER NOT NULL,
    token_address   TEXT NOT NULL,
    created_at      INTEGER NOT NULL,
    expires_at      INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_quotes_expires ON quotes(expires_at);

CREATE TABLE IF NOT EXISTS sessions (
    id              TEXT PRIMARY KEY,
    secret          TEXT NOT NULL,
    payer_address   TEXT NOT NULL,
    deposit_tx      TEXT NOT NULL,
    nonce           TEXT NOT NULL,
    deposit_amount  INTEGER NOT NULL,
    balance         INTEGER NOT NULL,
    rate_per_request INTEGER NOT NULL,
    requests_made   INTEGER NOT NULL DEFAULT 0,
    created_at      INTEGER NOT NULL,
    expires_at      INTEGER NOT NULL,
    status          TEXT NOT NULL DEFAULT 'active'
);
CREATE INDEX IF NOT EXISTS idx_sessions_payer ON sessions(payer_address);

CREATE TABLE IF NOT EXISTS request_log (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    payment_id      TEXT,
    session_id      TEXT,
    endpoint        TEXT NOT NULL,
    payer_address   TEXT NOT NULL,
    amount_charged  INTEGER NOT NULL,
    upstream_status INTEGER,
    upstream_latency_ms INTEGER,
    created_at      INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_request_log_created ON request_log(created_at);
CREATE INDEX IF NOT EXISTS idx_request_log_payer ON request_log(payer_address);
