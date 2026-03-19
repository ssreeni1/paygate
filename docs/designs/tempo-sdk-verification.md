# Tempo SDK API Verification Report

**Date**: 2026-03-19
**Purpose**: Verify that Tempo blockchain APIs referenced in SPEC.md actually exist post-mainnet launch (2026-03-18).

---

## 1. viem/tempo Exports

### 1.1 `Account.fromSecp256k1`

**Verdict: EXISTS**

- Exported from `viem/tempo` as part of the `Account` namespace
- Usage: `Account.fromSecp256k1('0xprivatekey...')` — creates a root account from a secp256k1 private key
- Import: `import { Account } from 'viem/tempo'`
- Source: [viem.sh/tempo](https://viem.sh/tempo), [GitHub wevm/viem src/tempo/index.ts](https://github.com/wevm/viem/blob/main/src/tempo/index.ts)

**SPEC match**: Correct as written in SPEC §7.1.

### 1.2 `tempoActions()`

**Verdict: EXISTS**

- Exported from `viem/tempo` (lowercase `tempoActions` is the function, `TempoActions` is the type/decorator)
- Extends a viem client with Tempo-specific properties: `calls` (batching), `feePayer` (sponsorship), `nonceKey` (concurrency)
- Import: `import { tempoActions } from 'viem/tempo'`
- Source: [viem.sh/tempo](https://viem.sh/tempo), [GitHub wevm/viem src/tempo/index.ts](https://github.com/wevm/viem/blob/main/src/tempo/index.ts)

**SPEC match**: Correct as written in SPEC §7.1.

### 1.3 `withFeePayer`

**Verdict: EXISTS**

- Exported from `viem/tempo`
- It is a **transport wrapper**, not a transaction property: `withFeePayer(http(), http('https://sponsor-url'))`
- Public testnet sponsor service: `https://sponsor.moderato.tempo.xyz`
- Import: `import { withFeePayer } from 'viem/tempo'`
- Source: [Tempo docs: Sponsor User Fees](https://docs.tempo.xyz/guide/payments/sponsor-user-fees), [GitHub wevm/viem src/tempo/index.ts](https://github.com/wevm/viem/blob/main/src/tempo/index.ts)

**SPEC match**: SPEC §4.4 usage is correct. The transport wrapping pattern `withFeePayer(http(), http('sponsor-url'))` matches the real API.

---

## 2. viem/chains Exports

### 2.1 `tempoTestnet`

**Verdict: EXISTS (but aliased)**

- `tempoTestnet` is an alias for `tempoAndantino` (which is deprecated)
- The **current** testnet is `tempoModerato` (chain ID 42431)
- `tempoAndantino` is marked deprecated in favor of `tempoModerato`
- Additional chain exports: `tempoDevnet`, `tempoLocalnet`
- Import: `import { tempoTestnet } from 'viem/chains'` — works but may resolve to deprecated Andantino
- Source: [GitHub wevm/viem src/chains/index.ts](https://github.com/wevm/viem/blob/main/src/chains/index.ts)

**SPEC impact**: SPEC §7.1 uses `tempoTestnet` which works but is an alias for the deprecated Andantino chain. **Consider using `tempoModerato` directly** for the current testnet, or `tempo` for mainnet.

### 2.2 `tempoMainnet`

**Verdict: DOES NOT EXIST (use `tempo` instead)**

- The mainnet chain export is simply **`tempo`**, not `tempoMainnet`
- Chain ID: **4217**
- RPC: `https://rpc.presto.tempo.xyz`
- WebSocket: `wss://rpc.presto.tempo.xyz`
- Native currency: USD (6 decimals)
- Explorer: `https://explore.tempo.xyz`
- Import: `import { tempo } from 'viem/chains'`
- Source: [GitHub wevm/viem src/chains/definitions/tempo.ts](https://github.com/wevm/viem/blob/main/src/chains/definitions/tempo.ts)

**SPEC impact**: SPEC §7.1 comment says `// or tempoMainnet when available` — this is **wrong**. The mainnet export is `tempo`, not `tempoMainnet`. Update SPEC §7.1.

---

## 3. TIP-20 Token Standard

### 3.1 `Transfer(address,address,uint256)` event

**Verdict: EXISTS**

- Standard ERC-20 compatible Transfer event
- `event Transfer(address indexed from, address indexed to, uint256 value)`
- Source: [TIP-20 Spec](https://docs.tempo.xyz/protocol/tip20/spec)

**SPEC match**: Correct as written.

### 3.2 `TransferWithMemo` event

**Verdict: EXISTS (but signature differs from SPEC)**

- Real signature: `event TransferWithMemo(address indexed from, address indexed to, uint256 value, bytes32 indexed memo)`
- **Key difference**: `memo` is **indexed** in the real spec
- Source: [Tempo docs: Transfer Memos](https://docs.tempo.xyz/guide/payments/transfer-memos), [TIP-20 Spec](https://docs.tempo.xyz/protocol/tip20/spec)

**SPEC impact**: The ITIP20.sol interface in the contracts has `memo` as non-indexed. The real TIP-20 spec has `memo` as **indexed**. This matters for log filtering — indexed parameters are topic fields, not data fields. **Update contracts/src/interfaces/ITIP20.sol** and the gateway's event decoding logic.

### 3.3 `transferWithMemo` function

**Verdict: EXISTS**

- `function transferWithMemo(address to, uint256 amount, bytes32 memo) external returns (bool)`
- Memo is a fixed 32-byte field
- Source: [TIP-20 Spec](https://docs.tempo.xyz/protocol/tip20/spec)

**SPEC match**: Correct as written.

---

## 4. Tempo RPC

### 4.1 Mainnet RPC URL

**Verdict: EXISTS (but differs from SPEC)**

- Real mainnet RPC: **`https://rpc.presto.tempo.xyz`**
- WebSocket: `wss://rpc.presto.tempo.xyz`
- Source: [GitHub wevm/viem tempo.ts chain definition](https://github.com/wevm/viem/blob/main/src/chains/definitions/tempo.ts)

**SPEC impact**: SPEC §5.1 uses `https://rpc.tempo.xyz` as the default RPC URL. The real mainnet RPC is `https://rpc.presto.tempo.xyz`. **Update SPEC §5.1 config defaults.**

### 4.2 Chain ID

**Verdict: EXISTS**

- Mainnet chain ID: **4217**
- Testnet (Moderato) chain ID: **42431**
- Source: [GitHub wevm/viem chain definitions](https://github.com/wevm/viem/blob/main/src/chains/definitions/tempo.ts)

**SPEC impact**: SPEC §5.1 has `chain_id = 0` as placeholder. **Update to 4217 for mainnet default.**

### 4.3 `tempo_fundAddress` RPC method

**Verdict: EXISTS**

- Used for testnet faucet: `cast rpc tempo_fundAddress <ADDRESS> --rpc-url https://rpc.moderato.tempo.xyz`
- Only available on testnet RPC endpoints
- Provides test stablecoins (pathUSD, AlphaUSD, BetaUSD, ThetaUSD)
- Source: [Tempo docs: Faucet](https://docs.tempo.xyz/quickstart/faucet)

**SPEC match**: Not explicitly referenced in SPEC but relevant for T24 (e2e testnet test).

---

## 5. Machine Payments Protocol (MPP)

### 5.1 Formal MPP wire spec

**Verdict: EXISTS (and differs significantly from SPEC)**

- The MPP spec **has been published** at [mpp.dev](https://mpp.dev/overview) and [github.com/tempoxyz/mpp-specs](https://github.com/tempoxyz/mpp-specs)
- MPP uses the **`Payment` HTTP authentication scheme**, NOT `X-Payment-*` custom headers
- Challenge: `WWW-Authenticate: Payment ...` (server → client)
- Credential: `Authorization: Payment ...` (client → server)
- This is a standard HTTP auth scheme, not custom headers
- Source: [GitHub tempoxyz/mpp-specs](https://github.com/tempoxyz/mpp-specs), [mpp.dev](https://mpp.dev/overview)

**SPEC impact**: **MAJOR DIVERGENCE**. The entire PayGate header format in SPEC §4.1 uses `X-Payment-*` custom headers. The real MPP uses `WWW-Authenticate: Payment` / `Authorization: Payment` standard HTTP auth scheme. The spec architecture (Core, Intents, Methods, Extensions) is modular. **SPEC §4.1, §4.2, §4.3, and all header references need a complete rewrite to align with MPP.**

### 5.2 `tempo curl`

**Verdict: DOES NOT EXIST (use `npx mppx` instead)**

- There is no `tempo curl` command
- The real CLI tool is **`npx mppx`** — handles 402 → sign → retry automatically
- Setup: `npx mppx account create`
- Usage: `npx mppx https://api.example.com --method POST -J '{"key":"value"}'`
- Source: [Parallel MPP docs](https://docs.parallel.ai/integrations/tempo-mpp)

**SPEC impact**: SPEC §7.4 references `tempo curl` which does not exist. **Replace with `npx mppx` reference.**

---

## Summary of Required SPEC Updates

### Critical (breaks implementation)

| # | Issue | SPEC Section | Fix |
|---|-------|-------------|-----|
| 1 | **MPP uses `WWW-Authenticate: Payment` / `Authorization: Payment`, NOT `X-Payment-*` headers** | §4.1, §4.2, §4.3, §4.4 | Rewrite 402 response format and payment verification to use MPP auth scheme |
| 2 | **Mainnet RPC is `https://rpc.presto.tempo.xyz`**, not `https://rpc.tempo.xyz` | §5.1 | Update `rpc_urls` default |
| 3 | **Mainnet chain ID is 4217** | §5.1 | Update `chain_id` from placeholder 0 |
| 4 | **`TransferWithMemo` event has `memo` as `indexed`** | §4.2, contracts | Update event decoding to read `memo` from topics, not data |

### Moderate (incorrect references)

| # | Issue | SPEC Section | Fix |
|---|-------|-------------|-----|
| 5 | Mainnet chain export is `tempo`, not `tempoMainnet` | §7.1 | Change comment to `// or tempo for mainnet` |
| 6 | `tempoTestnet` is deprecated alias for Andantino; current testnet is `tempoModerato` | §7.1 | Use `tempoModerato` or note the alias |
| 7 | `tempo curl` does not exist; CLI is `npx mppx` | §7.4 | Replace `tempo curl` section with `npx mppx` |

### Low (nice to update)

| # | Issue | SPEC Section | Fix |
|---|-------|-------------|-----|
| 8 | Testnet RPC is `https://rpc.moderato.tempo.xyz` | §5.1 | Add as testnet default |
| 9 | `tempo_fundAddress` RPC exists for testnet faucet | §16 | Reference in T24 test setup |
| 10 | Open Question #3 (MPP spec) is now resolved | §15 | Mark as resolved, link to mpp.dev |

---

## Sources

- [Viem Tempo Getting Started](https://viem.sh/tempo)
- [Viem Tempo Account.fromP256](https://viem.sh/tempo/accounts/account.fromP256)
- [GitHub wevm/viem src/tempo/index.ts](https://github.com/wevm/viem/blob/main/src/tempo/index.ts)
- [GitHub wevm/viem src/chains/index.ts](https://github.com/wevm/viem/blob/main/src/chains/index.ts)
- [GitHub wevm/viem tempo.ts chain definition](https://github.com/wevm/viem/blob/main/src/chains/definitions/tempo.ts)
- [GitHub wevm/viem tempoModerato.ts](https://github.com/wevm/viem/blob/main/src/chains/definitions/tempoModerato.ts)
- [TIP-20 Token Standard Overview](https://docs.tempo.xyz/protocol/tip20/overview)
- [TIP-20 Spec](https://docs.tempo.xyz/protocol/tip20/spec)
- [Tempo Transfer Memos](https://docs.tempo.xyz/guide/payments/transfer-memos)
- [Tempo Connection Details](https://docs.tempo.xyz/quickstart/connection-details)
- [Tempo Faucet](https://docs.tempo.xyz/quickstart/faucet)
- [Tempo Sponsor User Fees](https://docs.tempo.xyz/guide/payments/sponsor-user-fees)
- [GitHub tempoxyz/mpp-specs](https://github.com/tempoxyz/mpp-specs)
- [MPP Overview](https://mpp.dev/overview)
- [Parallel MPP Integration](https://docs.parallel.ai/integrations/tempo-mpp)
- [Stripe MPP Announcement](https://stripe.com/blog/machine-payments-protocol)
- [Fortune: Tempo MPP Launch](https://fortune.com/2026/03/18/stripe-tempo-paradigm-mpp-ai-payments-protocol/)
