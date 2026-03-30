# Design System — Agent Tips

## Product Context
- **What this is:** A consumer web experience where AI agents autonomously tip open-source developers. The receipt page IS the viral asset.
- **Who it's for:** OSS developers (GitHub-native, craft-minded) and Twitter viewers (need to instantly understand what happened)
- **Space/industry:** OSS funding, crypto payments, agent-to-human economics
- **Project type:** Consumer web app (receipt pages, profiles, claim flow, leaderboard, badges)

## Aesthetic Direction
- **Direction:** Terminal monochrome. ASCII box-drawing. Text-mode artifact.
- **Mood:** A receipt printed by a machine. Quiet, precise, austere. Not trying to be pretty. The content does the work. Like reading a bank statement from a future where AI agents have accounts.
- **Anti-patterns:** No gradients. No colored backgrounds. No rounded cards. No icons. No decorative elements. No marketing language. The only color is the accent on amounts.

## Typography
- **Everything is mono.** JetBrains Mono primary, Geist Mono fallback, system monospace.
- **No display fonts.** No sans-serif body. One font family, one voice.
- **Scale:** 11px labels, 13px small, 14px body, 16px large, 18px section, 24px title, 40px hero

## Color
- **Approach:** Monochrome with one accent
- **Background:** `#0C0C0C`
- **Surface:** `#111111`
- **Border:** `#333333`, subtle `#222222`
- **Text:** primary `#E0E0E0`, muted `#888888`, dim `#555555`
- **Accent:** `#B4F0A0` (soft green, used ONLY for amounts and success states)
- **Warning:** `#F0D080` (amber, for escrowed/pending)
- **Error:** `#F09090`
- **No other colors.** Everything else is grayscale.

## Layout
- **No border-radius anywhere.** Square corners only.
- **ASCII box-drawing characters** for decorative framing (┌─┐ └─┘)
- **Dashed separators** using ASCII (── ── ── ──)
- **Max widths:** receipt 560px, content 640px, leaderboard 800px

## Elements
- **Status indicators:** ● paid, ○ escrowed, ✓ claimed, × reclaimed (text, not badges)
- **Buttons:** border only, no fill. Hover changes text + border to accent.
- **Links:** no underline by default. Hover shows accent color.
- **Inputs:** border only, no rounded corners, dark background

## Motion
- **One animation:** `print-in` (subtle translateY 4px + fade, 300ms). Used once on the receipt amount. Nothing else moves.

## Decisions Log
| Date | Decision | Rationale |
|------|----------|-----------|
| 2026-03-30 | Replaced cyan/Satoshi/DM Sans design with terminal monochrome | User wanted "more minimalistic and cool, not a standard tech page, incorporate ASCII elements" |
| 2026-03-30 | Single mono font family (JetBrains Mono) | Everything being mono creates the terminal feel. Multiple fonts break the illusion. |
| 2026-03-30 | Accent color #B4F0A0 (soft green) instead of cyan | Reads as terminal/matrix without being cliche. Only used on amounts. |
| 2026-03-30 | ASCII box-drawing for receipt framing | The receipt literally looks like terminal output. Distinctive in any Twitter feed. |
