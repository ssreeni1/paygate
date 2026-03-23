# Prompt: Build Transaction Explorer Frontend

You are building the frontend for the PayGate transaction explorer feature.

## Instructions

1. Read the build brief:
   - `/Users/saneel/projects/paygate/worktree-briefs/pane-tx-frontend-brief.md`

2. Read these source files for context:
   - `/Users/saneel/projects/paygate/docs/marketplace.html` — you will modify this file to add the transaction feed section and JS
   - `/Users/saneel/projects/paygate/docs/style.css` — you will append transaction feed styles

3. Implement all changes described in the brief:
   - Add the live transactions HTML section to `marketplace.html` (between the API cards section and the `<!-- TEMPO -->` bar)
   - Add the new stats (payments count, revenue, pulse dot) to the existing `.mp-stats-bar` in the stats section
   - Add all the transaction feed JavaScript to the existing `<script>` block (polling, rendering, sound toggle, Blockscout fallback)
   - Append all transaction feed CSS to `style.css`

4. Verify:
   - Open `marketplace.html` in a browser and confirm:
     - The transaction feed section renders below the API cards
     - Skeleton loading animation shows initially
     - Empty state shows if no transactions (expected for dev)
     - Sound toggle button appears and toggles between muted/unmuted icons
     - Stats bar shows the new payment count and revenue placeholders
     - No JS console errors
   - Check responsive layout at mobile widths (< 768px)

5. Commit with message:
   ```
   feat: add live transaction feed to marketplace with auto-refresh, sound, and Blockscout fallback
   ```

## Important notes

- All JavaScript is inline in the `<script>` block — no external JS files
- Reuse existing helper functions: `escapeHtml()`, `API_BASE` constant
- The endpoint pill colors must match the API card border colors (green/blue/purple/orange)
- Sound is OFF by default — user must click to enable (never auto-play audio)
- The Blockscout fallback URL uses the actual provider address `0x002925FAFE98cfeB9fdBb7d6045ce318E4BD4b88`
- USDC has 6 decimals: divide raw amount by 1_000_000 for display
- Truncate addresses to `0x1234...abcd` format (first 6 + last 4 characters)
- All new CSS classes use the `tx-` prefix to avoid conflicts with existing styles
- Keep the dark theme consistent: #0d1117 page bg, #161b22 card bg, #30363d borders
