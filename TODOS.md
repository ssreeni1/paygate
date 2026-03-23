# TODOS

## Blocking for Production

(none — all blockers resolved)

## Non-blocking

### Split main.rs into modules
- **What:** Extract CLI commands, gateway handler, and tests from main.rs (~1900 LOC) into separate files.
- **Why:** main.rs does too much — CLI arg parsing, gateway handler, middleware wiring, all test functions. Hard to navigate, will get worse with Wave 2 features.
- **Context:** Retro flagged this. Extract `cli/serve.rs`, `cli/init.rs`, `cli/revenue.rs`, etc.
- **Depends on:** Nothing.

### Publish create-paygate to npm
- **What:** Publish the `packages/create-paygate` package to npm so `npx create-paygate` works.
- **Why:** The landing page and docs tell people to run `npx create-paygate` but it doesn't exist on npm yet.
- **Depends on:** npm account setup.

### Record demo video
- **What:** 30-second terminal recording of an agent using the marketplace — discover, pay, get results.
- **Why:** The launch content for Twitter/HN. The landing page has a hero section ready for video embed.
- **Depends on:** Nothing (marketplace is live with real payments).

### Dashboard design specification (v0.3)
- **What:** Full design specification for the React revenue dashboard — screens, data visualization, interactions, responsive behavior. Run `/design-consultation` before implementation.
- **Why:** Currently described as "React revenue analytics" with zero design detail. Without design work, it will ship as generic AI-generated dashboard slop.
- **Pros:** Ensures the dashboard has design intentionality and matches the CLI's minimal/professional tone.
- **Cons:** Deferred until v0.3 — no immediate cost.
- **Context:** The design review (DESIGN-REVIEW.md) explicitly flagged this as deferred. CLI conventions are defined but dashboard has no design system yet.
- **Depends on:** v0.2 completion.

### MPP wire protocol header compatibility
- **What:** When Tempo publishes the formal Machine Payments Protocol (MPP) spec, audit PayGate's `X-Payment-*` headers for compatibility with the standard. Update headers to match if needed.
- **Why:** `tempo curl` is documented as handling MPP 402 negotiation automatically. If PayGate's headers diverge from the MPP spec, `tempo curl` won't work with PayGate gateways.
- **Pros:** Interoperability with the Tempo ecosystem tooling; broader adoption.
- **Cons:** May require breaking change to header format (versioned migration).
- **Context:** As of 2026-03-18, the MPP wire spec has not been published. Track at Tempo GitHub. PayGate currently uses custom `X-Payment-*` headers defined in SPEC.md Section 4.1. The spec already notes this as an open question (line 102).
- **Depends on:** Tempo publishing MPP specification.

### Rust client SDK (Wave 2)
- **What:** Implement Rust client SDK (paygate-client crate).
- **Why:** TS SDK works and is testnet-verified. Rust client is secondary — most consumers will use TS.
- **Depends on:** Nothing.

## Completed

### Resolve Tempo mainnet chain configuration
- **Status:** DONE — chain ID 4217, RPC `rpc.presto.tempo.xyz`, updated in code.

### Verify Tempo SDK APIs before testnet integration
- **Status:** DONE — full report at `docs/designs/tempo-sdk-verification.md`, testnet E2E passed.

### Quickstart documentation for payment flow
- **Status:** DONE v0.2.0 — `quickstart.html` live at `ssreeni1.github.io/paygate/quickstart` with sidebar nav, SDK guide, manual curl guide, testnet setup.

### Deploy docs site for help_url
- **Status:** DONE v0.2.0 — GitHub Pages auto-deploys from `docs/` directory. All 402 `help_url` links resolve.

### Railway RPC connectivity
- **Status:** DONE v0.3.0 — Node.js RPC proxy at `localhost:3001/rpc` relays Tempo calls for the Rust gateway. 6 real payments verified on deployed Railway instance.
