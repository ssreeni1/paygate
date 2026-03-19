# Prompt: Build Demo Marketplace APIs

## Instructions

You are building the demo marketplace server for PayGate — 4 upstream API wrappers in a Node.js Express server.

### Step 1: Read the brief and existing code

Read these files for full context:
- `/Users/saneel/projects/paygate/worktree-briefs/pane-demo-apis-brief.md` (your build brief)
- `/Users/saneel/projects/paygate/SPEC.md` (sections 4.1 for 402 response format, 5.1 for config)
- `/Users/saneel/projects/paygate/paygate.toml.example` (existing example config — your demo needs its own)

### Step 2: Build everything

Create the `demo/` directory at the repo root with all files specified in the brief:
- `package.json` with all dependencies
- `tsconfig.json` (strict mode)
- `src/server.ts` — Express app with all 5 routes
- `src/routes/` — one file per endpoint (pricing, search, scrape, image, summarize)
- `src/lib/` — shared upstream client, error handling, semaphore, validation
- `tests/` — vitest tests for each endpoint (mock upstream calls)
- `Dockerfile` + `entrypoint.sh`
- `paygate.toml` — config for the demo instance (pricing for all 5 endpoints)

### Step 3: Run tests

```bash
cd demo && npm install && npm test
```

Fix any failures before proceeding.

### Step 4: Verify

- Confirm the server starts: `cd demo && npm run dev` (it will fail on missing env vars — that's correct behavior, verify the error message is clear)
- Confirm TypeScript compiles: `cd demo && npx tsc --noEmit`

### Step 5: Commit

Stage all new files in `demo/` and commit:
```
feat(demo): add marketplace demo server with 4 API wrappers

Express server wrapping Brave Search, Playwright scraper, Replicate SDXL,
and Anthropic Haiku behind PayGate pricing. Includes vitest tests, Dockerfile,
and paygate.toml for the demo instance.
```
