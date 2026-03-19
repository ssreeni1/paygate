# Changelog

## [0.1.0] - 2026-03-19

### Added

- Single Rust binary gateway (`paygate serve`) with axum + tower middleware stack
- TOML configuration with static per-endpoint pricing and single accepted token
- Config validation at startup with descriptive error messages
- Config hot-reload via SIGHUP with ArcSwap
- 402 Payment Required responses with quote IDs, TTL, human-readable `message` and `help_url`
- On-chain payment verification via Tempo RPC (TIP-20 Transfer event log decoding)
- RPC failover with `rpc_urls` array and connection pooling
- Payer binding — `X-Payment-Payer` must match on-chain Transfer `from` address
- Request hash computation (`keccak256(method || path || body)`) binding payment to specific request
- Payment memo verification (`keccak256("paygate" || quoteId || requestHash)`)
- Replay protection via SQLite UNIQUE constraint on `tx_hash`
- Transaction age check (reject stale transactions beyond `tx_expiry_seconds`)
- Ambiguous transaction rejection (multiple matching Transfer events)
- SQLite database with WAL mode, dedicated writer task, batch writes (10ms/50-write flush)
- Bounded write channel with backpressure (503 on overflow)
- Rate limiting — global and per-payer request limits, 402 flood protection
- Free endpoint passthrough (price == 0 skips payment middleware)
- `paygate init` — 3-question interactive setup wizard
- `paygate serve` — gateway startup with structured JSON logging and graceful shutdown
- `paygate status` — component health dashboard (gateway, upstream, RPC, DB)
- `paygate pricing` — endpoint pricing table display
- `paygate pricing --html` — static HTML pricing page generator
- `paygate revenue` — revenue summary for 24h, 7d, 30d with top endpoints
- `paygate wallet` — provider on-chain balance + 24h income summary
- `paygate test` — end-to-end verification against Tempo testnet
- `paygate demo` — self-contained demo with built-in echo server
- `paygate sessions` — active session listing
- TypeScript client SDK (`@paygate/sdk`) with auto-pay 402 negotiation
- Rust client SDK (`paygate-client`) with auto-discovery
- Health check endpoint (`GET /paygate/health`) with per-component status
- Prometheus metrics endpoint (`GET /paygate/metrics`) — payments, verification latency, upstream latency, revenue, rate limits, RPC errors, DB errors, webhook delivery, config reloads
- Receipt verification endpoint (`GET /paygate/receipts/{tx_hash}`)
- `X-Payment-Receipt` and `X-Payment-Cost` response headers on successful payment
- Webhook notification on payment verified (fire-and-forget, HTTPS-only, SSRF-safe)
- Request logging with configurable retention and periodic cleanup
- Graceful shutdown on SIGTERM/SIGINT with 30s drain timeout
- Defensive error handling for null receipts, malformed event logs, disk full, and upstream response overflow
- Shared request hash test vectors (`tests/fixtures/request_hash_vectors.json`) for cross-language parity
- CLI output conventions: 2-space indent, `───` underlines, `error:` + `hint:` format, NO_COLOR support
