# Design System — Agent Tips

## Product Context
- **What this is:** A consumer web experience where AI agents autonomously tip open-source developers. The receipt page IS the viral asset.
- **Who it's for:** OSS developers (GitHub-native, craft-minded) and Twitter viewers (need to instantly understand what happened)
- **Space/industry:** OSS funding, crypto payments, agent-to-human economics
- **Project type:** Consumer web app (receipt pages, profiles, claim flow, leaderboard, badges)

## Aesthetic Direction
- **Direction:** Retro-Futuristic Industrial
- **Decoration level:** Intentional. Monospace accents for data. Subtle grid/scan-line texture on receipt backgrounds. Nothing else.
- **Mood:** A dispatch from the future where AI agents are economic actors. Machine precision everywhere except where the human shows up. Not cold cyberpunk, not warm cozy. Sharp, precise, with one moment of warmth: the human receiving the tip.
- **Reference sites:** x402.org (minimal, confident), Stripe receipts (clean data hierarchy)

## Typography
- **Display/Hero:** Satoshi (clean geometric sans, modern but not generic)
- **Body:** DM Sans (clean, readable, pairs well with Satoshi)
- **UI/Labels:** DM Sans (same as body)
- **Data/Tables:** Geist Mono (tabular-nums, tx hashes, amounts. Pairs with Satoshi/DM Sans)
- **Code:** Geist Mono
- **Loading:** Google Fonts for Satoshi fallback (Fontshare primary), Google Fonts for DM Sans + Geist Mono
- **Scale:** 11px (labels/mono) / 13px (small body) / 14px (body) / 16px (large body) / 18px (subtitle) / 22px (section) / 32px (page title) / 48px (display) / 56px (hero amount)

## Color
- **Approach:** Restrained with one electric accent
- **Background:** `#0A0A0B` (near-black canvas)
- **Surface:** `#141416` (cards, receipt background)
- **Surface hover:** `#1C1C1F`
- **Border:** `#27272A` (zinc-800, subtle separation)
- **Border subtle:** `#1E1E21`
- **Primary text:** `#FAFAFA` (zinc-50)
- **Muted text:** `#A1A1AA` (zinc-400)
- **Dim text:** `#71717A` (zinc-500)
- **Accent (cyan):** `#22D3EE` (amounts, key actions, tip indicator)
- **Accent dim:** `rgba(34, 211, 238, 0.15)` (backgrounds, highlights)
- **Accent glow:** `rgba(34, 211, 238, 0.3)` (human avatar ring)
- **Semantic:** success `#4ADE80`, warning `#FBBF24`, error `#F87171`, info `#38BDF8`
- **Dark mode:** Dark-first. This IS the dark mode. Light mode deferred.

## Spacing
- **Base unit:** 4px
- **Density:** Comfortable
- **Scale:** 2xs(2) xs(4) sm(8) md(16) lg(24) xl(32) 2xl(48) 3xl(64)

## Layout
- **Approach:** Grid-disciplined
- **Grid:** Single column centered for receipt/claim, 2-column for leaderboard on desktop
- **Max content width:** 640px for receipts/profiles, 1100px for leaderboard
- **Border radius:** sm:4px, md:8px, lg:12px, full:9999px (status badges)

## Motion
- **Approach:** Minimal-functional
- **Easing:** enter(ease-out) exit(ease-in) move(ease-in-out)
- **Duration:** micro(50-100ms for hover states) short(150ms for transitions) medium(300ms for amount count-up)
- **Specific animations:**
  - Amount count-up on receipt page load (0.00 -> 0.50, 300ms)
  - Status badge fade-in (150ms)
  - Card entrance on scroll (subtle translateY 8px, 200ms)
  - Nothing else. Content does the work.

## Component Patterns

### Receipt Card
- Centered, max-width 520px
- 2px cyan gradient line at top
- Amount: hero-sized monospace in cyan
- Reason: italic pull-quote with left cyan border
- Parties: sender (dashed border avatar) -> recipient (cyan glow ring avatar)
- Meta: key-value rows in small monospace

### OG Card (1200x630)
- Dark surface background
- 2px cyan gradient line at top
- Large amount in cyan
- "Agent tipped @user for package" as the headline
- Status badge bottom-left, brand bottom-right
- Must be readable at thumbnail size

### Status Badges
- Pill-shaped (border-radius: full)
- Monospace, uppercase, small
- Paid: green text on green-dim background
- Escrowed: amber text on amber-dim background

### Human Avatar Treatment
- Circular, with cyan glow ring (box-shadow: 0 0 0 3px cyan-dim, 0 0 20px cyan-glow)
- This is the ONE warm element. Everything else is machine-precise.
- Agent avatars get a dashed border instead (no glow)

### Badge (SVG for README)
- Two-part: "agent tips" label (dark bg) + "$X from Y agents" value (with cyan text)
- shields.io style, fits README conventions

## Decisions Log
| Date | Decision | Rationale |
|------|----------|-----------|
| 2026-03-30 | Initial design system created | Created by /design-consultation. Dark-first, retro-futuristic industrial. Cyan accent chosen over blue/purple to differentiate from every other crypto/OSS product. Receipt-as-poster over receipt-as-form to maximize viral shareability. |
| 2026-03-30 | Satoshi + DM Sans + Geist Mono | Satoshi for display impact, DM Sans for body clarity, Geist Mono for data precision. Deliberately avoided Inter/Roboto/Poppins. |
| 2026-03-30 | Human avatar cyan glow | The contrast between machine precision and human warmth IS the product story. The glow ring makes the human the center of attention. |
