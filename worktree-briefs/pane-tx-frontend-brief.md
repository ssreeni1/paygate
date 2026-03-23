# Build Brief: Transaction Explorer — Frontend

## Overview

Add a live transaction feed to the marketplace page that shows recent verified payments, auto-refreshes every 10 seconds, plays an optional sound on new transactions, and falls back to Blockscout if the gateway API is unavailable.

## Changes

### 1. marketplace.html — Transaction feed section

Add a new section between the API cards section and the Tempo bar. Insert it right before the `<!-- TEMPO -->` comment.

#### Structure

```html
<!-- LIVE TRANSACTIONS -->
<section class="tx-feed-section fade-in">
  <div class="wrap">
    <div class="tx-feed-header">
      <h2>Live Transactions</h2>
      <button class="sound-toggle" id="sound-toggle" title="Toggle transaction sound">🔇</button>
    </div>
    <div id="tx-feed" class="tx-feed">
      <div id="tx-loading" class="tx-loading">
        <div class="skeleton-row"></div>
        <div class="skeleton-row"></div>
        <div class="skeleton-row"></div>
      </div>
      <div id="tx-empty" class="tx-empty" style="display:none;">
        No transactions yet. Be the first &mdash; try an API above.
      </div>
      <div id="tx-rows" class="tx-rows"></div>
    </div>
    <div id="tx-fallback-note" class="tx-fallback-note" style="display:none;">
      Live feed from blockchain explorer &mdash; gateway data unavailable
    </div>
  </div>
</section>
```

#### Transaction row

Each row renders as:

```html
<div class="tx-row tx-row-enter">
  <span class="tx-time">2m ago</span>
  <span class="tx-endpoint-pill" style="background:rgba(63,185,80,0.15);color:#3fb950;">search</span>
  <span class="tx-amount">0.002000 USDC</span>
  <span class="tx-payer">0x1234...abcd</span>
  <a class="tx-verify-link" href="https://explore.moderato.tempo.xyz/tx/0x..." target="_blank" rel="noopener">Verify →</a>
</div>
```

Endpoint pill colors (match API card border colors):
- `search` -> green (#3fb950)
- `scrape` -> blue (#58a6ff)
- `image` -> purple (#d2a8ff)
- `summarize` -> orange (#f0883e)
- fallback -> gray (#8b949e)

#### JavaScript logic

Add to the `<script>` block:

```javascript
// --- Transaction Feed ---
const TX_URL = `${API_BASE}/paygate/transactions`;
const BLOCKSCOUT_FALLBACK = 'https://explore.moderato.tempo.xyz/api/v2/addresses/0x002925FAFE98cfeB9fdBb7d6045ce318E4BD4b88/token-transfers?type=ERC-20&token=0x20c0000000000000000000000000000000000000';
const EXPLORER_BASE = 'https://explore.moderato.tempo.xyz/tx/';

let lastTxHash = null;
let soundEnabled = false;

// Endpoint -> pill color mapping
function endpointColor(endpoint) {
  if (!endpoint) return { bg: 'rgba(139,148,158,0.15)', fg: '#8b949e' };
  const e = endpoint.toLowerCase();
  if (e.includes('search')) return { bg: 'rgba(63,185,80,0.15)', fg: '#3fb950' };
  if (e.includes('scrape')) return { bg: 'rgba(88,166,255,0.15)', fg: '#58a6ff' };
  if (e.includes('image'))  return { bg: 'rgba(210,168,255,0.15)', fg: '#d2a8ff' };
  if (e.includes('summarize')) return { bg: 'rgba(240,136,62,0.15)', fg: '#f0883e' };
  return { bg: 'rgba(139,148,158,0.15)', fg: '#8b949e' };
}

// Endpoint slug extraction: "POST /v1/search" -> "search"
function endpointSlug(endpoint) {
  if (!endpoint) return 'tx';
  return endpoint.split('/').pop() || 'tx';
}

// Relative time: "2m ago", "1h ago", "3d ago"
function timeAgo(timestamp) {
  const now = Math.floor(Date.now() / 1000);
  const diff = now - timestamp;
  if (diff < 60) return `${diff}s ago`;
  if (diff < 3600) return `${Math.floor(diff / 60)}m ago`;
  if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`;
  return `${Math.floor(diff / 86400)}d ago`;
}

// Truncate address: "0x1234567890abcdef..." -> "0x1234...cdef"
function truncAddr(addr) {
  if (!addr || addr.length < 12) return addr || '';
  return addr.slice(0, 6) + '...' + addr.slice(-4);
}

// Format amount: integer base units (6 decimals for USDC) -> "0.002000"
function fmtAmount(amount) {
  return (amount / 1_000_000).toFixed(6);
}

// Build a transaction row element
function buildTxRow(tx) {
  const color = endpointColor(tx.endpoint);
  const slug = endpointSlug(tx.endpoint);
  const div = document.createElement('div');
  div.className = 'tx-row tx-row-enter';
  div.innerHTML = `
    <span class="tx-time">${timeAgo(tx.verified_at)}</span>
    <span class="tx-endpoint-pill" style="background:${color.bg};color:${color.fg};">${escapeHtml(slug)}</span>
    <span class="tx-amount">${fmtAmount(tx.amount)} USDC</span>
    <span class="tx-payer">${truncAddr(tx.payer_address)}</span>
    <a class="tx-verify-link" href="${EXPLORER_BASE}${encodeURIComponent(tx.tx_hash)}" target="_blank" rel="noopener">Verify &rarr;</a>
  `;
  return div;
}

// Sound: synthesize a short coin chime via Web Audio API
function playChime() {
  if (!soundEnabled) return;
  try {
    const ctx = new (window.AudioContext || window.webkitAudioContext)();
    const osc = ctx.createOscillator();
    const gain = ctx.createGain();
    osc.connect(gain);
    gain.connect(ctx.destination);
    osc.type = 'sine';
    osc.frequency.setValueAtTime(1200, ctx.currentTime);
    osc.frequency.exponentialRampToValueAtTime(800, ctx.currentTime + 0.15);
    gain.gain.setValueAtTime(0.15, ctx.currentTime);
    gain.gain.exponentialRampToValueAtTime(0.001, ctx.currentTime + 0.3);
    osc.start(ctx.currentTime);
    osc.stop(ctx.currentTime + 0.3);
  } catch (e) { /* ignore audio errors */ }
}

// Sound toggle
document.getElementById('sound-toggle').addEventListener('click', function() {
  soundEnabled = !soundEnabled;
  this.textContent = soundEnabled ? '🔊' : '🔇';
  localStorage.setItem('paygate-sound', soundEnabled ? '1' : '0');
});

// Restore sound preference
if (localStorage.getItem('paygate-sound') === '1') {
  soundEnabled = true;
  document.getElementById('sound-toggle').textContent = '🔊';
}

// Pulse dot in stats bar
function flashPulseDot() {
  let dot = document.getElementById('tx-pulse-dot');
  if (!dot) return;
  dot.classList.remove('pulsing');
  void dot.offsetWidth; // force reflow
  dot.classList.add('pulsing');
}

// Update stats bar with payment count + revenue
function updateStats(total, totalRevenue) {
  const countEl = document.getElementById('mp-stat-payments');
  const revenueEl = document.getElementById('mp-stat-revenue');
  if (countEl) countEl.innerHTML = `<strong style="color:#3fb950">${total}</strong> payments`;
  if (revenueEl) revenueEl.innerHTML = `<strong style="color:#3fb950">$${(totalRevenue / 1_000_000).toFixed(2)}</strong> earned`;
}

// Fetch from gateway
async function fetchTransactions() {
  try {
    const resp = await fetch(`${TX_URL}?limit=20`);
    if (!resp.ok) throw new Error(`HTTP ${resp.status}`);
    const data = await resp.json();
    renderTransactions(data.transactions || [], false);
    updateStats(data.total || 0, data.total_revenue || 0);
    return true;
  } catch (e) {
    console.warn('Gateway tx fetch failed, trying Blockscout:', e);
    return false;
  }
}

// Fallback: fetch from Blockscout
async function fetchBlockscout() {
  try {
    const resp = await fetch(BLOCKSCOUT_FALLBACK);
    if (!resp.ok) throw new Error(`HTTP ${resp.status}`);
    const data = await resp.json();
    const items = (data.items || []).slice(0, 20).map(item => ({
      tx_hash: item.tx_hash || '',
      payer_address: item.from?.hash || '',
      amount: parseInt(item.total?.value || '0', 10),
      endpoint: null,
      verified_at: item.timestamp ? Math.floor(new Date(item.timestamp).getTime() / 1000) : 0,
      status: 'verified'
    }));
    renderTransactions(items, true);
    return true;
  } catch (e) {
    console.warn('Blockscout fallback also failed:', e);
    return false;
  }
}

// Render transaction rows
function renderTransactions(txs, isFallback) {
  const loading = document.getElementById('tx-loading');
  const empty = document.getElementById('tx-empty');
  const rows = document.getElementById('tx-rows');
  const note = document.getElementById('tx-fallback-note');

  loading.style.display = 'none';

  if (isFallback) {
    note.style.display = 'block';
  }

  if (txs.length === 0) {
    empty.style.display = 'block';
    rows.innerHTML = '';
    return;
  }

  empty.style.display = 'none';

  // Detect new transactions
  const newFirstHash = txs[0]?.tx_hash;
  const isNew = lastTxHash !== null && newFirstHash !== lastTxHash;

  if (isNew) {
    playChime();
    flashPulseDot();
  }

  lastTxHash = newFirstHash;

  // Rebuild rows
  rows.innerHTML = '';
  txs.forEach(tx => {
    rows.appendChild(buildTxRow(tx));
  });
}

// Poll loop
async function pollTransactions() {
  const ok = await fetchTransactions();
  if (!ok) await fetchBlockscout();
}

// Start polling
pollTransactions();
setInterval(pollTransactions, 10_000);
```

### 2. Stats bar update

Modify the existing stats bar in marketplace.html. Add two new stat spans and a pulse dot inside the `.mp-stats-bar` div:

```html
<span class="mp-stat" id="mp-stat-payments"><strong style="color:#3fb950">—</strong> payments</span>
<span class="mp-stat" id="mp-stat-revenue"><strong style="color:#3fb950">—</strong> earned</span>
<span class="mp-stat"><span class="pulse-dot" id="tx-pulse-dot"></span></span>
```

Insert these after the existing "4 APIs" and "DEMO mode" stats, before the `mp-stat-note` span.

### 3. Sound toggle

- Button in the feed header (see HTML above): `🔇` by default
- Click toggles `soundEnabled` boolean
- Stores preference in `localStorage` key `paygate-sound` (`'1'` or `'0'`)
- Restores on page load
- Sound synthesized via Web Audio API (see JS above): short descending sine tone, 0.3s duration, gentle volume

### 4. Blockscout fallback

If `GET /paygate/transactions` fails (network error or non-200):
- Fetch from: `https://explore.moderato.tempo.xyz/api/v2/addresses/0x002925FAFE98cfeB9fdBb7d6045ce318E4BD4b88/token-transfers?type=ERC-20&token=0x20c0000000000000000000000000000000000000`
- Parse Blockscout response: map `items[]` to transaction objects (tx_hash, payer from `from.hash`, amount from `total.value`, no endpoint name)
- Show amber note: "Live feed from blockchain explorer — gateway data unavailable"
- Endpoint pill shows gray "tx" for Blockscout-sourced rows (no endpoint info available)

### 5. style.css — Add transaction feed styles

Append to the end of style.css, before the final `@media` block or at the very end:

```css
/* ========================================
   Transaction Feed
   ======================================== */

.tx-feed-section {
  padding: 40px 0;
}

.tx-feed-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 1rem;
}

.tx-feed-header h2 {
  margin-bottom: 0;
}

.tx-feed {
  background: #161b22;
  border: 1px solid #30363d;
  border-radius: 10px;
  overflow: hidden;
}

/* Transaction rows */
.tx-row {
  display: flex;
  align-items: center;
  gap: 1rem;
  padding: 12px 20px;
  border-bottom: 1px solid #21262d;
  font-size: 0.875rem;
  transition: background 0.15s;
}

.tx-row:last-child {
  border-bottom: none;
}

.tx-row:hover {
  background: #1c2128;
}

.tx-time {
  color: #8b949e;
  font-size: 0.8rem;
  min-width: 60px;
  flex-shrink: 0;
}

.tx-endpoint-pill {
  font-family: 'JetBrains Mono', monospace;
  font-size: 0.7rem;
  font-weight: 600;
  padding: 2px 8px;
  border-radius: 4px;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  white-space: nowrap;
}

.tx-amount {
  font-family: 'JetBrains Mono', monospace;
  font-weight: 600;
  color: #3fb950;
  white-space: nowrap;
}

.tx-payer {
  font-family: 'JetBrains Mono', monospace;
  font-size: 0.8rem;
  color: #8b949e;
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
}

.tx-verify-link {
  color: #3fb950 !important;
  font-size: 0.8rem;
  white-space: nowrap;
  text-decoration: none;
  flex-shrink: 0;
}

.tx-verify-link:hover {
  color: #56d364 !important;
  text-decoration: underline;
}

/* Slide-in animation for new rows */
.tx-row-enter {
  animation: txSlideIn 0.4s ease-out;
}

@keyframes txSlideIn {
  from {
    opacity: 0;
    transform: translateY(-12px);
  }
  to {
    opacity: 1;
    transform: translateY(0);
  }
}

/* Pulse dot */
.pulse-dot {
  display: inline-block;
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: #3fb950;
  opacity: 0.4;
}

.pulse-dot.pulsing {
  animation: pulseDot 0.6s ease-out 3;
}

@keyframes pulseDot {
  0% { opacity: 1; transform: scale(1.5); box-shadow: 0 0 8px rgba(63, 185, 80, 0.6); }
  100% { opacity: 0.4; transform: scale(1); box-shadow: none; }
}

/* Skeleton loading rows */
.tx-loading {
  padding: 0;
}

.skeleton-row {
  height: 44px;
  margin: 0;
  border-bottom: 1px solid #21262d;
  background: linear-gradient(90deg, #161b22 25%, #1c2128 50%, #161b22 75%);
  background-size: 200% 100%;
  animation: shimmer 1.5s infinite;
}

.skeleton-row:last-child {
  border-bottom: none;
}

@keyframes shimmer {
  0% { background-position: 200% 0; }
  100% { background-position: -200% 0; }
}

/* Empty state */
.tx-empty {
  padding: 2rem 1.25rem;
  text-align: center;
  color: #8b949e;
  font-size: 0.9rem;
}

/* Fallback note */
.tx-fallback-note {
  margin-top: 0.75rem;
  padding: 0.6rem 1rem;
  background: rgba(240, 136, 62, 0.1);
  border: 1px solid rgba(240, 136, 62, 0.3);
  border-radius: 6px;
  font-size: 0.8rem;
  color: #f0883e;
}

/* Sound toggle button */
.sound-toggle {
  background: #21262d;
  border: 1px solid #30363d;
  border-radius: 6px;
  padding: 4px 10px;
  font-size: 1rem;
  cursor: pointer;
  transition: background 0.15s;
  line-height: 1;
}

.sound-toggle:hover {
  background: #30363d;
}

/* Responsive: feed rows stack on mobile */
@media (max-width: 768px) {
  .tx-row {
    flex-wrap: wrap;
    gap: 0.5rem;
    padding: 10px 16px;
  }

  .tx-time {
    min-width: auto;
  }

  .tx-payer {
    flex-basis: 100%;
    order: 10;
  }

  .tx-verify-link {
    margin-left: auto;
  }
}
```

## Commit message

```
feat: add live transaction feed to marketplace with auto-refresh, sound, and Blockscout fallback
```

## Key patterns to follow

- **HTML structure**: Follow the existing section pattern (`.fade-in` wrapper, `.wrap` inner container)
- **JavaScript**: All JS is inline in the `<script>` block at the bottom of marketplace.html. Use the existing `API_BASE`, `escapeHtml()`, and fetch patterns.
- **CSS class naming**: Follow the existing `mp-` prefix pattern for marketplace-specific styles, and use `tx-` prefix for transaction feed styles
- **Colors**: Green (#3fb950), blue (#58a6ff), purple (#d2a8ff), orange (#f0883e) — matching the API card borders
- **Dark theme**: All backgrounds use #0d1117 (page), #161b22 (cards), #1c2128 (code blocks), borders #30363d
- **Font**: JetBrains Mono for monospace elements (amounts, addresses, pills)
