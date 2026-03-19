# Prompt: Build create-paygate + paygate register

## Instructions

You are building two CLI tools: the `create-paygate` npm scaffolding package and the `paygate register` Rust subcommand.

### Step 1: Read the brief and existing code

Read these files for full context:
- `/Users/saneel/projects/paygate/worktree-briefs/pane-cli-tools-brief.md` (your build brief)
- `/Users/saneel/projects/paygate/SPEC.md` (section 6.1 for PayGateRegistry contract, section 9 for CLI conventions)
- `/Users/saneel/projects/paygate/crates/paygate-gateway/src/main.rs` (existing Commands enum, CLI structure, prompt helper)
- `/Users/saneel/projects/paygate/crates/paygate-gateway/src/config.rs` (config loading, parse_price_to_base_units)
- `/Users/saneel/projects/paygate/contracts/src/PayGateRegistry.sol` (registerService function signature, ServiceRegistered event)
- `/Users/saneel/projects/paygate/crates/paygate-gateway/Cargo.toml` (existing dependencies — check for alloy crates)
- `/Users/saneel/projects/paygate/sdk/testnet-e2e.mjs` (existing E2E test — extend with register step)

### Step 2: Build create-paygate

Create `packages/create-paygate/` with all files from the brief:
- `package.json` (name: "create-paygate", bin entry)
- `tsconfig.json`
- `src/index.ts` — CLI entrypoint with `#!/usr/bin/env node`
- `src/wizard.ts` — interactive prompts using the `prompts` package
- `src/scaffold.ts` — generates files into target directory
- `src/templates/` — template functions for each generated file
- `tests/scaffold.test.ts` — verify scaffold output
- `tests/wizard.test.ts` — verify input validation

Then install and test:
```bash
cd packages/create-paygate && npm install && npm test
```

### Step 3: Build paygate register

1. Add `Register` variant to `Commands` enum in `main.rs`
2. Implement `cmd_register` async function
3. Use alloy's `sol!` macro (or manual ABI encoding) for the `registerService` call
4. Add any needed alloy dependencies to `Cargo.toml`
5. Follow the existing CLI output style (see `cmd_init`, `cmd_status` for patterns)

Then build and test:
```bash
cd /Users/saneel/projects/paygate && cargo build -p paygate-gateway
cargo test -p paygate-gateway
```

### Step 4: Extend E2E test (optional)

In `sdk/testnet-e2e.mjs`, add a step 8 (after the existing step 7) that:
1. Calls `paygate register` via the CLI (or documents how to test it manually)
2. This is optional — only add if it can work without a deployed registry contract

### Step 5: Verify

- `npx create-paygate` should work from the packages directory: `cd packages/create-paygate && node dist/index.js test-project`
- `cargo build -p paygate-gateway` compiles with the new Register command
- All existing tests still pass

### Step 6: Commit

Two separate commits:

First:
```
feat(create-paygate): add npx create-paygate scaffolding tool

Interactive wizard that generates paygate.toml, sample Express server,
Dockerfile, README, and .env.example for new PayGate-wrapped APIs.
```

Second:
```
feat(cli): add paygate register subcommand

Registers a service in the on-chain PayGateRegistry contract via
Tempo RPC. Encodes registerService call, sends transaction, and
prints the serviceId with explorer link.
```
