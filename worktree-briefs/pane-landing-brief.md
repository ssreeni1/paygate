# Brief: Landing Page + Analytics

## Objective

Build the product landing page for PayGate at `docs/index.html`, hosted on GitHub Pages at `ssreeni1.github.io/paygate`. Add Plausible analytics to all HTML pages.

## Files

```
docs/
  index.html           # Main landing page (NEW — replace if redirect exists)
  quickstart.html       # Keep existing, add Plausible script
  style.css             # Shared stylesheet (NEW)
```

## Design Direction

- Clean, professional, developer-focused
- Dark terminal-style hero section (dark background, monospace text, subtle glow)
- Light content sections below
- No JavaScript frameworks — vanilla HTML + CSS only
- Mobile-responsive with flexbox/grid (optimized for desktop, works on phone)
- No emoji in copy. Use sharp, confident language.
- Total page weight < 100KB (no images besides favicon; inline SVG for any icons)

## Page Sections (in order)

### 1. Hero

**Background**: Dark (#0d1117 or similar GitHub-dark), gradient or subtle noise texture in CSS.

**Content**:
```
The API marketplace where
agents don't need API keys.

Wrap any API behind per-request stablecoin payments.
AI agents discover, pay, and use your API autonomously.

[Try the Demo]    [Add Your API]
```

Below the text, show a terminal-style box (CSS only, no JS animation needed — static is fine):

```
$ curl https://demo-paygate.fly.dev/v1/search \
    -d '{"query": "latest AI news"}'

HTTP/1.1 402 Payment Required
X-Payment-Amount: 2000
X-Payment-Token: USDC

{"error": "payment_required", "pricing": {"amount": "0.002", ...}}
```

The terminal box should have:
- Dark background (#161b22)
- Monospace font
- Subtle border or shadow
- "Terminal title bar" with 3 dots (red/yellow/green circles in CSS)

### 2. How It Works

Three-column layout (stacks on mobile):

**Step 1: Request**
```
Agent sends a request to your API.
PayGate returns 402 with the price.
```

**Step 2: Pay**
```
Agent sends USDC on Tempo (sub-second finality).
Fee sponsorship means zero gas needed.
```

**Step 3: Done**
```
Agent retries with payment proof.
PayGate verifies and proxies the request.
```

Each step has a step number (1, 2, 3) in a circle, a title, and 2 lines of description.

Optionally: a simple ASCII-art or SVG flow diagram showing `Agent -> 402 -> Pay -> 200`.

### 3. Try It Now

Light background section.

```
Try it now

The demo marketplace is live on Tempo testnet.
Send a real request — see the 402 flow in action.
```

Code block with a curl command:
```bash
curl -X POST https://demo-paygate.fly.dev/v1/search \
  -H "Content-Type: application/json" \
  -d '{"query": "latest AI news"}'
```

Note: Use `https://demo-paygate.fly.dev` as the placeholder URL. This will be updated when the demo is actually deployed.

### 4. Available APIs

Grid of 4 cards (2x2 on desktop, 1-column on mobile):

**Web Search** — $0.002/request
POST /v1/search
Powered by Brave Search API.
```json
{"query": "AI news"} → {"results": [{"title": "...", "url": "..."}]}
```

**Web Scrape** — $0.001/request
POST /v1/scrape
Returns clean markdown from any URL.
```json
{"url": "https://..."} → {"markdown": "# Article Title\n..."}
```

**Image Generation** — $0.01/request
POST /v1/image
SDXL via Replicate.
```json
{"prompt": "a cat astronaut"} → {"image_url": "https://..."}
```

**Text Summarize** — $0.003/request
POST /v1/summarize
Claude Haiku.
```json
{"text": "Long article..."} → {"summary": "Key points..."}
```

Each card:
- White background, subtle border, slight shadow
- Endpoint in monospace
- Price prominent
- Compact example request/response

### 5. Add Your API

Dark background section (matches hero).

```
Add your API to the marketplace

Three commands. Sixty seconds. Real payments.
```

```bash
npx create-paygate my-api
cd my-api
npm start
```

Then three bullets:
- `paygate.toml` — configure pricing and your wallet address
- `Dockerfile` — deploy to fly.io, railway, or any container host
- PayGate handles payments, verification, and proxying

### 6. For Agent Developers

Light background.

```
For agent developers

One SDK. Automatic payment negotiation.
Your agent handles 402s without human intervention.
```

```typescript
import { PayGateClient } from '@paygate/sdk';

const client = new PayGateClient({ tempoClient });

// Automatic: discovers price, pays, retries
const result = await client.fetch(
  'https://demo-paygate.fly.dev/v1/search',
  { method: 'POST', body: JSON.stringify({ query: 'AI news' }) }
);
```

```bash
npm install @paygate/sdk
```

### 7. Built on Tempo

One line, centered, subtle:

```
Built on Tempo — sub-second finality, stablecoin-native, Stripe-backed.
```

Link "Tempo" to `https://tempo.xyz`.

### 8. Footer

Simple footer:
- Left: "PayGate" + "Open source API payment gateway"
- Right: links to GitHub repo, Documentation (quickstart.html), Tempo
- Very minimal, no newsletter signup or social links

## CSS (docs/style.css)

### Typography
- Body: `system-ui, -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif`
- Code: `'SF Mono', 'Fira Code', 'JetBrains Mono', monospace`
- Base size: 16px
- Line height: 1.6 for body, 1.4 for headings

### Colors
- Dark sections: bg `#0d1117`, text `#e6edf3`, accent `#58a6ff`
- Light sections: bg `#ffffff`, text `#1f2937`, accent `#2563eb`
- Code blocks in dark sections: bg `#161b22`, border `#30363d`
- Code blocks in light sections: bg `#f6f8fa`, border `#d0d7de`
- Card borders: `#e5e7eb`

### Layout
- Max content width: 1100px, centered
- Section padding: 80px vertical, 24px horizontal
- Card grid: `grid-template-columns: repeat(auto-fit, minmax(280px, 1fr))`, gap 24px

### Responsive
- Below 768px: single column, reduced padding (40px vertical)
- Below 480px: further reduced font sizes
- Terminal box: horizontal scroll if content overflows

### Code Blocks
- Padding: 16px 20px
- Border radius: 8px
- Overflow-x: auto
- Syntax highlighting: not required (keep it simple), but use color for the prompt `$` and comments

### Buttons
- Primary: solid accent color, white text, rounded corners
- Secondary: outline style, accent color border and text
- Both: padding 12px 24px, hover state with slight darken/lighten

## Plausible Analytics

Add to the `<head>` of BOTH `index.html` and `quickstart.html`:

```html
<script defer data-domain="ssreeni1.github.io" src="https://plausible.io/js/script.js"></script>
```

This is the Plausible Cloud script — no self-hosting needed. It tracks page views automatically with no cookies.

## Existing quickstart.html

Read `docs/quickstart.html` to understand its current content. Add:
1. The Plausible script tag to `<head>`
2. A link to `<link rel="stylesheet" href="style.css">` if it would benefit from shared styles (or leave its existing styles if they work)
3. A "Back to home" link at the top pointing to `index.html`

## HTML Structure

```html
<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>PayGate — The API marketplace where agents don't need API keys</title>
  <meta name="description" content="Wrap any API behind per-request stablecoin payments. AI agents discover, pay, and use your API autonomously.">
  <link rel="stylesheet" href="style.css">
  <script defer data-domain="ssreeni1.github.io" src="https://plausible.io/js/script.js"></script>
</head>
<body>
  <header class="hero">...</header>
  <section class="how-it-works">...</section>
  <section class="try-it">...</section>
  <section class="apis">...</section>
  <section class="add-api">...</section>
  <section class="sdk">...</section>
  <section class="tempo">...</section>
  <footer>...</footer>
</body>
</html>
```

Use semantic HTML. No divitis. Minimal nesting.

## Open Graph / Social Meta

Add to `<head>` for social sharing:
```html
<meta property="og:title" content="PayGate — API marketplace for AI agents">
<meta property="og:description" content="Wrap any API behind per-request stablecoin payments. AI agents discover, pay, and use your API autonomously.">
<meta property="og:type" content="website">
<meta property="og:url" content="https://ssreeni1.github.io/paygate">
<meta name="twitter:card" content="summary_large_image">
<meta name="twitter:title" content="PayGate — API marketplace for AI agents">
<meta name="twitter:description" content="Per-request stablecoin payments for APIs. No API keys. No human signup.">
```

## Key Constraints

- Pure HTML + CSS. No JavaScript besides Plausible (which is deferred and non-blocking).
- No build step. Files are served directly by GitHub Pages.
- Page must load fast (< 100KB total, no external fonts -- use system fonts).
- All code examples must be accurate and match the actual API shapes from the demo brief.
- Placeholder URL `https://demo-paygate.fly.dev` used throughout -- easy to find-and-replace later.
- The page should look good enough to share on Twitter/X and Hacker News. First impressions matter.
