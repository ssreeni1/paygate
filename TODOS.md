# TODOS

## Blocking for Production

### Resolve Tempo mainnet chain configuration
- **What:** Research and lock in Tempo mainnet chain ID, RPC URL, and USDC token contract address.
- **Why:** All payment verification depends on correct chain config. Building against testnet for development, but real money requires mainnet values.
- **Pros:** Unblocks production deployment; catches any mainnet-vs-testnet behavioral differences early.
- **Cons:** Blocked on Tempo publishing stable mainnet docs/chain objects.
- **Context:** Tempo mainnet launched 2026-03-18. The spec uses placeholder values (`chain_id = 0`, `rpc_url = "https://rpc.tempo.xyz"`). Check `viem/chains` for a `tempoMainnet` export, and `docs.tempo.xyz` for RPC endpoints and token registry.
- **Depends on:** Tempo mainnet documentation being published.

### Quickstart documentation for payment flow
- **What:** Write the page at `ssreeni1.github.io/paygate/quickstart#paying` — the URL linked from every 402 response's `help_url` field.
- **Why:** Every 402 response includes `"help_url": "https://ssreeni1.github.io/paygate/quickstart#paying"`. Without actual docs at that URL, developers hit a dead link at the exact moment they need help. This is a pre-launch requirement.
- **Pros:** Completes the DX loop; developers can self-serve from the 402 response.
- **Cons:** Requires a docs site (GitHub Pages, Vercel, etc.) — minimal setup.
- **Context:** The 402 JSON body format was finalized in the design review (DESIGN-REVIEW.md §4). The page should cover: what 402 means, how to send a TIP-20 payment, how to retry with X-Payment-Tx header, and how to use the SDK for auto-pay.
- **Depends on:** Finalized 402 response format (done), working payment flow.

### Verify Tempo SDK APIs before testnet integration
- **What:** Confirm all `viem/tempo` exports referenced in SPEC.md and the TypeScript SDK actually exist: `Account.fromSecp256k1`, `tempoActions()`, `withFeePayer`, TIP-20 event ABIs, `tempo_fundAddress` RPC method. Update code if any differ.
- **Why:** The SDK was built against pre-launch API docs. Actual exports may differ. The `paygate test` and `paygate demo` commands depend on these APIs working.
- **Pros:** Prevents runtime failures on first testnet run.
- **Cons:** None — this is a prerequisite for testnet integration.
- **Context:** Tempo mainnet launched 2026-03-18. The TS SDK (`sdk/src/client.ts`) imports from `viem/tempo` and `viem/chains`. Gateway RPC calls in `verifier.rs` use standard `eth_getTransactionReceipt`. Check actual package exports against code imports.
- **Depends on:** Nothing — can be done immediately.

### Deploy docs site for help_url
- **What:** Set up GitHub Pages at `ssreeni1.github.io/paygate` and publish the quickstart page. The quickstart content is partially covered by README.md now, but the `help_url` in 402 responses points to `https://ssreeni1.github.io/paygate/quickstart#paying` which must be a live URL.
- **Why:** Every 402 response links to this URL. Dead links at the moment a developer needs help is a terrible first impression.
- **Pros:** Completes the DX loop from 402 → docs → payment → success.
- **Cons:** Requires DNS setup and a deployment pipeline (minimal with GitHub Pages).
- **Context:** README.md now has the "How it works" and SDK sections that can be adapted for the quickstart page. The 402 JSON format is finalized in DESIGN-REVIEW.md §4.
- **Depends on:** Quickstart documentation (above).

## Non-blocking

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
