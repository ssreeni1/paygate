You are investigating whether the Tempo blockchain SDK APIs referenced in PayGate's SPEC.md actually exist.

This is a critical pre-deployment verification task. The spec was generated from pre-launch docs by an agent. Tempo mainnet launched 2026-03-18. APIs may have changed.

Read SPEC.md sections 7.1 (TypeScript SDK), 4.4 (fee sponsorship), and 15 (open questions) to understand what APIs are referenced.

Then use WebSearch to verify each of these APIs exists:

1. **viem/tempo exports:**
   - `Account.fromSecp256k1` — does this exist in viem's Tempo module?
   - `tempoActions()` — is this a real viem extension?
   - `withFeePayer` — does this transport wrapper exist?
   - Search: "viem tempo" site:viem.sh OR site:github.com/wevm/viem

2. **viem/chains exports:**
   - `tempoTestnet` — is there a Tempo testnet chain object in viem?
   - `tempoMainnet` — is there a mainnet chain object yet?
   - Search: "viem chains tempo" OR check viem docs

3. **TIP-20 token standard:**
   - Is `Transfer(address,address,uint256)` the standard event (same as ERC-20)?
   - Does `TransferWithMemo` exist? What's the exact event signature?
   - Search: "TIP-20 tempo token standard" OR "tempo transferWithMemo"

4. **Tempo RPC:**
   - What is the actual mainnet RPC URL?
   - What is the chain ID?
   - Does `tempo_fundAddress` RPC method exist for the testnet faucet?
   - Search: "tempo blockchain rpc" site:docs.tempo.xyz OR site:tempo.xyz

5. **Machine Payments Protocol (MPP):**
   - Has the formal MPP wire spec been published?
   - Does `tempo curl` exist and handle 402 negotiation?
   - Search: "machine payments protocol tempo" OR "tempo curl mpp"

For each API, report:
- EXISTS / DOES NOT EXIST / CANNOT CONFIRM
- If it exists but differs from the spec, describe the difference
- Link to the source (docs URL, GitHub file, npm package)

Write your findings to docs/designs/tempo-sdk-verification.md with clear verdicts for each API.

If any API does NOT exist or differs significantly, list the spec sections that need updating.

This is research only — do NOT modify any source code.
