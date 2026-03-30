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
    deposit_tx      TEXT UNIQUE NOT NULL,
    nonce           TEXT UNIQUE NOT NULL,
    deposit_amount  INTEGER NOT NULL,
    balance         INTEGER NOT NULL,
    rate_per_request INTEGER NOT NULL,
    requests_made   INTEGER NOT NULL DEFAULT 0,
    created_at      INTEGER NOT NULL,
    expires_at      INTEGER NOT NULL,
    status          TEXT NOT NULL DEFAULT 'active'
);
CREATE INDEX IF NOT EXISTS idx_sessions_payer ON sessions(payer_address);

CREATE TABLE IF NOT EXISTS session_nonces (
    nonce       TEXT PRIMARY KEY,
    payer_address TEXT NOT NULL,
    created_at  INTEGER NOT NULL,
    expires_at  INTEGER NOT NULL,
    consumed    INTEGER NOT NULL DEFAULT 0
);

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

-- Agent Tips: tip records (paid or escrowed)
CREATE TABLE IF NOT EXISTS tips (
    id            TEXT PRIMARY KEY,
    sender_wallet TEXT NOT NULL,
    sender_name   TEXT,
    recipient_gh  TEXT NOT NULL,
    package_name  TEXT,
    amount_usdc   INTEGER NOT NULL,
    reason        TEXT NOT NULL,
    evidence      TEXT,
    status        TEXT NOT NULL DEFAULT 'escrowed',
    tx_hash       TEXT,
    claim_wallet  TEXT,
    created_at    TEXT NOT NULL DEFAULT (datetime('now')),
    claimed_at    TEXT,
    expires_at    TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_tips_recipient ON tips(recipient_gh) WHERE status = 'escrowed';
CREATE INDEX IF NOT EXISTS idx_tips_expires ON tips(expires_at) WHERE status = 'escrowed';
CREATE INDEX IF NOT EXISTS idx_tips_sender ON tips(sender_wallet);

-- Agent Tips: npm package → GitHub owner resolution cache
CREATE TABLE IF NOT EXISTS npm_cache (
    package_name TEXT PRIMARY KEY,
    github_owner TEXT NOT NULL,
    github_repo  TEXT,
    resolved_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Agent Tips: GitHub username → wallet address registry
CREATE TABLE IF NOT EXISTS tip_registry (
    github_username TEXT PRIMARY KEY,
    wallet_address  TEXT NOT NULL,
    registered_at   TEXT NOT NULL DEFAULT (datetime('now'))
);
