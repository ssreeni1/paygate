# Brief: Demo Marketplace APIs

## Objective

Build a Node.js Express server in `demo/` that wraps 4 upstream APIs. PayGate sits in front of this server and handles all payment. The demo server itself has no payment logic -- it just calls upstreams and returns results.

## Directory Structure

```
demo/
  package.json
  tsconfig.json
  src/
    server.ts          # Express app, route wiring, startup
    routes/
      pricing.ts       # GET /v1/pricing
      search.ts        # POST /v1/search
      scrape.ts        # POST /v1/scrape
      image.ts         # POST /v1/image
      summarize.ts     # POST /v1/summarize
    lib/
      upstream.ts      # Shared HTTP client (node-fetch wrapper with timeout, retries)
      errors.ts        # Error classes and middleware
      semaphore.ts     # Concurrency limiter for Playwright
      validation.ts    # Input validation helpers
  tests/
    search.test.ts
    scrape.test.ts
    image.test.ts
    summarize.test.ts
    pricing.test.ts
  Dockerfile
```

## package.json

```json
{
  "name": "paygate-demo",
  "version": "0.1.0",
  "private": true,
  "scripts": {
    "build": "tsc",
    "start": "node dist/server.js",
    "dev": "tsx src/server.ts",
    "test": "vitest run",
    "test:watch": "vitest"
  },
  "dependencies": {
    "express": "^4.18",
    "node-fetch": "^3.3",
    "playwright": "^1.42",
    "@mozilla/readability": "^0.5",
    "turndown": "^7.1",
    "jsdom": "^24.0"
  },
  "devDependencies": {
    "typescript": "^5.4",
    "tsx": "^4.7",
    "vitest": "^1.3",
    "@types/express": "^4.17",
    "@types/turndown": "^5.0",
    "supertest": "^6.3",
    "@types/supertest": "^6.0"
  }
}
```

## Routes

### GET /v1/pricing (free endpoint)

Returns the full API catalog. No authentication or payment needed. PayGate config must mark `GET /v1/pricing` as price `0.000`.

Response:
```json
{
  "apis": [
    {
      "endpoint": "POST /v1/search",
      "price": "0.002",
      "currency": "USDC",
      "description": "Web search powered by Brave Search API",
      "example_request": {"query": "latest AI news", "count": 5},
      "example_response_fields": ["title", "url", "description"]
    },
    {
      "endpoint": "POST /v1/scrape",
      "price": "0.001",
      "currency": "USDC",
      "description": "Web scraper — returns clean markdown from any URL",
      "example_request": {"url": "https://example.com"},
      "example_response_fields": ["title", "content", "markdown"]
    },
    {
      "endpoint": "POST /v1/image",
      "price": "0.01",
      "currency": "USDC",
      "description": "Image generation via Replicate SDXL",
      "example_request": {"prompt": "a cat astronaut, digital art"},
      "example_response_fields": ["image_url", "model", "generation_time_ms"]
    },
    {
      "endpoint": "POST /v1/summarize",
      "price": "0.003",
      "currency": "USDC",
      "description": "Text summarization via Claude Haiku",
      "example_request": {"text": "Long article text...", "max_length": 200},
      "example_response_fields": ["summary", "model", "input_tokens", "output_tokens"]
    }
  ]
}
```

### POST /v1/search

Request body:
```json
{"query": "string (required)", "count": "number (optional, default 5, max 20)"}
```

Implementation:
1. Validate: `query` must be a non-empty string, `count` 1-20
2. Call Brave Search API: `GET https://api.search.brave.com/res/v1/web/search?q={query}&count={count}`
   - Header: `X-Subscription-Token: ${BRAVE_API_KEY}`
3. Transform response: extract `web.results[]` into `{title, url, description, age}`
4. Return `{results: [...], query, result_count}`

### POST /v1/scrape

Request body:
```json
{"url": "string (required, must start with http:// or https://)"}
```

Implementation:
1. Validate URL format
2. Acquire semaphore slot (max 3 concurrent Playwright instances)
3. Launch Playwright chromium (headless), navigate to URL with 15s timeout
4. Wait for `networkidle` or 10s, whichever first
5. Extract page HTML via `page.content()`
6. Parse with JSDOM + Readability to get article content
7. Convert to markdown with Turndown
8. Return `{title, content: markdownString, url, scraped_at: ISO8601}`
9. Always close browser in finally block, release semaphore

Playwright lifecycle: create a single browser instance at startup, create new pages per request (not new browsers). Close pages after each request.

### POST /v1/image

Request body:
```json
{
  "prompt": "string (required, max 500 chars)",
  "width": "number (optional, default 1024)",
  "height": "number (optional, default 1024)"
}
```

Implementation:
1. Validate prompt length
2. Call Replicate API: `POST https://api.replicate.com/v1/predictions`
   - Header: `Authorization: Bearer ${REPLICATE_API_TOKEN}`
   - Body: `{version: "stability-ai/sdxl:...", input: {prompt, width, height}}`
3. Poll prediction status until `succeeded` or `failed` (max 60s, poll every 2s)
4. Return `{image_url: output[0], prompt, model: "sdxl", generation_time_ms}`

### POST /v1/summarize

Request body:
```json
{
  "text": "string (required, max 100000 chars)",
  "max_length": "number (optional, default 200 words)"
}
```

Implementation:
1. Validate text is non-empty
2. Call Anthropic Messages API: `POST https://api.anthropic.com/v1/messages`
   - Headers: `x-api-key: ${ANTHROPIC_API_KEY}`, `anthropic-version: 2023-06-01`
   - Body: model `claude-3-5-haiku-20241022`, system prompt asking for summary of max_length words, user message is the text
3. Return `{summary, model, input_tokens, output_tokens}`

## Shared Infrastructure

### upstream.ts — HTTP client wrapper

```typescript
interface UpstreamOptions {
  timeout?: number;       // ms, default 30000
  retries?: number;       // default 0
  retryDelay?: number;    // ms, default 1000
}

async function upstreamFetch(url: string, init: RequestInit, opts?: UpstreamOptions): Promise<Response>
```

- On upstream 429: throw `UpstreamRateLimitError` (maps to 502 for the consumer)
- On timeout: throw `UpstreamTimeoutError` (maps to 504)
- On upstream 5xx: throw `UpstreamServerError` (maps to 502)
- On network error: throw `UpstreamUnavailableError` (maps to 502)

### errors.ts — Error handling middleware

Error classes:
- `ValidationError` (400)
- `UpstreamRateLimitError` (502 with message "upstream rate limited")
- `UpstreamTimeoutError` (504)
- `UpstreamServerError` (502)
- `UpstreamUnavailableError` (502)

Express error middleware at the end of the chain:
```typescript
app.use((err, req, res, next) => {
  // Map error class to status code
  // Log error
  // Return JSON {error: string, message: string}
})
```

### semaphore.ts — Concurrency limiter

Simple counting semaphore with `acquire()` (returns promise) and `release()`. Max concurrency: 3 for Playwright.

## Environment Variables

Required at startup (fail fast with clear message if missing):
- `BRAVE_API_KEY` — Brave Search API subscription token
- `REPLICATE_API_TOKEN` — Replicate API token
- `ANTHROPIC_API_KEY` — Anthropic API key

Optional:
- `PORT` — default 3001
- `PLAYWRIGHT_MAX_CONCURRENCY` — default 3

At startup, immediately check all 3 required keys exist. If any missing:
```
error: missing required environment variable: BRAVE_API_KEY
  hint: get a free API key at https://brave.com/search/api/
```

## Port and PayGate Integration

The demo server runs on port 3001. PayGate's `paygate.toml` is configured with:
```toml
[gateway]
upstream = "http://localhost:3001"

[pricing.endpoints]
"GET /v1/pricing" = "0.000"
"POST /v1/search" = "0.002"
"POST /v1/scrape" = "0.001"
"POST /v1/image" = "0.01"
"POST /v1/summarize" = "0.003"
```

## Tests (vitest + supertest)

Each test file should test against the Express app directly (no PayGate in the test loop).

For each endpoint:
1. **Happy path** — mock the upstream HTTP call (use vitest mocking for node-fetch), verify response shape
2. **Validation error** — send bad input, expect 400
3. **Upstream failure** — mock upstream returning 500, expect 502

For scrape specifically:
4. **Semaphore exhaustion** — mock Playwright to be slow, fire 4+ concurrent requests, verify 4th gets queued (not rejected)

Mock strategy: mock `node-fetch` at the module level using `vi.mock()`. For Playwright, mock the browser/page objects.

## Dockerfile

Multi-stage build that runs both the demo server and PayGate together:

```dockerfile
# Stage 1: Build PayGate binary
FROM rust:1.77-slim AS paygate-build
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/
COPY paygate-common/ paygate-common/  # if workspace member
RUN cargo build --release -p paygate-gateway

# Stage 2: Build demo server
FROM node:20-slim AS demo-build
WORKDIR /app
COPY demo/package.json demo/package-lock.json ./
RUN npm ci
COPY demo/ .
RUN npm run build
RUN npx playwright install --with-deps chromium

# Stage 3: Runtime
FROM node:20-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
    libnss3 libatk1.0-0 libatk-bridge2.0-0 libcups2 libdrm2 \
    libxkbcommon0 libxcomposite1 libxdamage1 libxrandr2 libgbm1 \
    libpango-1.0-0 libasound2 && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=paygate-build /build/target/release/paygate /usr/local/bin/paygate
COPY --from=demo-build /app/dist ./dist
COPY --from=demo-build /app/node_modules ./node_modules
COPY --from=demo-build /root/.cache/ms-playwright /root/.cache/ms-playwright
COPY demo/paygate.toml /app/paygate.toml
COPY entrypoint.sh /app/entrypoint.sh

RUN chmod +x /app/entrypoint.sh
EXPOSE 8080

CMD ["/app/entrypoint.sh"]
```

entrypoint.sh:
```bash
#!/bin/bash
set -e

# Start demo server in background
node /app/dist/server.js &
DEMO_PID=$!

# Wait for demo server to be ready
for i in $(seq 1 10); do
  curl -sf http://localhost:3001/v1/pricing && break
  sleep 1
done

# Start PayGate (foreground)
exec paygate serve --config /app/paygate.toml
```

## Key Constraints

- TypeScript strict mode
- No payment logic in the demo server -- PayGate handles all of that
- Every upstream call must have a timeout (30s default, 60s for image generation)
- Log all upstream calls with timing: `[search] query="AI news" upstream_ms=342 status=200`
- Playwright browser instance reuse (single browser, new context per request)
- Graceful shutdown: close Playwright browser on SIGTERM
