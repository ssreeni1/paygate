# Prompt: Build Fee Sponsorship Endpoint

## Instructions

You are implementing the Tempo fee payer service in the PayGate Rust gateway.

### Step 1: Read the brief and existing code

Read these files for full context:
- `/Users/saneel/projects/paygate/worktree-briefs/pane-sponsor-brief.md` (your build brief)
- `/Users/saneel/projects/paygate/SPEC.md` (section 4.4 for fee sponsorship flow, section 5.1 for config)
- `/Users/saneel/projects/paygate/crates/paygate-gateway/src/config.rs` (SponsorshipConfig — already exists with enabled, sponsor_listen, budget_per_day, max_per_tx)
- `/Users/saneel/projects/paygate/crates/paygate-gateway/src/server.rs` (AppState struct)
- `/Users/saneel/projects/paygate/crates/paygate-gateway/src/main.rs` (router setup in cmd_serve, Commands enum)
- `/Users/saneel/projects/paygate/crates/paygate-gateway/src/metrics.rs` (existing metric helpers)
- `/Users/saneel/projects/paygate/docs/designs/tempo-sdk-verification.md` (withFeePayer transport confirmed, public sponsor at sponsor.moderato.tempo.xyz)
- `/Users/saneel/projects/paygate/crates/paygate-gateway/Cargo.toml` (check existing alloy dependencies)

### Step 2: Research the Tempo fee payer protocol

Before writing code, understand the exact protocol:
- Study what `withFeePayer(http(), http('https://sponsor-url'))` sends to the sponsor URL
- The public testnet sponsor is at `https://sponsor.moderato.tempo.xyz` — examine what it accepts
- Check the viem/tempo source code for the `withFeePayer` transport implementation
- The sponsor endpoint likely acts as a JSON-RPC proxy that intercepts transaction signing

### Step 3: Build everything

1. Create `crates/paygate-gateway/src/sponsor.rs` with:
   - `SponsorService` struct (budget tracking, signer, HTTP client)
   - `SponsorBudget` with atomic daily tracking and midnight reset
   - Request handler that implements the Tempo fee payer protocol
   - Balance check background task (every 60s)

2. Update `crates/paygate-gateway/src/main.rs`:
   - Add `mod sponsor;`
   - Wire the sponsor endpoint into the gateway router (only when `config.sponsorship.enabled = true`)
   - Fail at startup if sponsorship enabled but private key missing

3. Add the `paygate_sponsor_budget_remaining` gauge metric

4. Add any needed dependencies to `Cargo.toml`

### Step 4: Run tests

```bash
cd /Users/saneel/projects/paygate && cargo test -p paygate-gateway
```

The brief specifies 5 test cases — implement all of them. Fix any failures.

### Step 5: Verify compilation

```bash
cargo build -p paygate-gateway
```

### Step 6: Commit

Stage changed and new files and commit:
```
feat(gateway): implement fee sponsorship endpoint (/paygate/sponsor)

Adds the Tempo fee payer HTTP service with daily budget tracking,
per-tx limits, wallet balance monitoring, and Prometheus metrics.
Wired into the gateway router when sponsorship.enabled = true.
```
