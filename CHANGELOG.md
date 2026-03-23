# Changelog

## [0.3.0] - 2026-03-23

### Added

- **Live transaction feed** on marketplace page — auto-refreshing every 10s with slide-in animations
- **GET /paygate/transactions** endpoint — returns recent payments with totals, revenue, and Blockscout explorer links
- **Marketplace UI page** (`marketplace.html`) — fetches live API data, interactive "Try it" panels that execute real 402 requests
- **⟨$⟩ logo** — dollar sign in green brackets, integrated across nav, footer, and favicon
- **Sound toggle** on transaction feed — opt-in coin chime on new payments via Web Audio API
- **Stats bar** with live payment count, revenue counter, and green pulse dot
- **Syntax highlighting** in code blocks (keywords coral, strings green)
- **Code language labels** on docs page (bash, typescript, http, pseudocode)
- **Scroll-active sidebar** in docs — highlights current section as you scroll
- **Copy-to-clipboard** buttons on all code blocks
- **CHEAPEST badge** on lowest-priced API card in marketplace

### Changed

- Landing page redesigned: tighter spacing, subtler section labels, hero glow line, card hover effects with colored shadows, left-aligned CTA
- API cards now 2x2 grid (was 3+1) with colored left borders (green/blue/purple/orange)
- Buttons have hover micro-interactions (lift + glow shadow)
- Terminal comments brightened from gray to muted green for readability

### Fixed

- **Node.js RPC proxy** unblocks Railway → Tempo connectivity (the root cause of all Railway payment verification failures since launch)
- CORS support added to gateway for cross-origin marketplace UI fetches
- Marketplace "See the live marketplace" button now opens the marketplace UI page instead of raw JSON

## [0.2.0] - 2026-03-19

### Added

- **Demo marketplace** — 4 API wrappers (search, scrape, image gen, summarize) with mock mode when API keys not configured
- **Fee sponsorship endpoint** (`/paygate/sponsor`) — Tempo fee payer protocol for gas-free consumer payments, with budget tracking and balance monitoring
- **`create-paygate` npm package** — interactive wizard scaffolds a complete PayGate project with config, Dockerfile, and sample server
- **`paygate register` CLI command** — register APIs in on-chain PayGateRegistry contract
- **Landing page** — story-driven product page with terminal demo, stats section, API cards, scroll animations, copy-to-clipboard, favicon, OG image template
- **Docs page** — sidebar navigation, section dividers, TL;DR callout, responsive mobile layout
- **Marketplace E2E test** — real on-chain payments through deployed Railway instance (3 APIs paid, replay protection verified)
- **Railway deployment** — Dockerfile with multi-stage build (Rust gateway + Node demo server), Railway-compatible with PORT handling
- **GitHub Pages** — auto-deploy workflow for landing page and docs

### Changed

- Landing page redesigned: full dark mode, green accent (#3fb950), JetBrains Mono headings, hero gradient glow, staggered card animations
- RPC startup check now non-fatal — gateway starts even if Tempo RPC is temporarily unreachable
- Demo server uses DEMO_PORT env var to avoid Railway PORT conflict
- Updated Tempo chain exports: `tempoModerato` (testnet), `tempo` (mainnet)
- ITIP20.sol: memo parameter now indexed (matches real TIP-20 spec)

### Fixed

- Memo decoded from indexed `topics[3]` instead of data field (caught by testnet E2E)
- Replay detection uses rusqlite error code instead of fragile string matching
- Receipt endpoint no longer leaks internal DB errors
- Config defaults updated: RPC `rpc.presto.tempo.xyz`, chain ID 4217
- `help_url` updated to real GitHub Pages domain
- Proxy strips hop-by-hop headers (transfer-encoding, content-length) to fix free endpoint passthrough
- HTML pricing page escapes values to prevent XSS
- 402 flood rate limiter wired into gateway handler
- `format_usd` uses integer math instead of floating point

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
