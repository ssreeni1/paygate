# Prompt: Build Landing Page + Analytics

## Instructions

You are building the product landing page for PayGate, hosted on GitHub Pages.

### Step 1: Read the brief and existing code

Read these files for full context:
- `/Users/saneel/projects/paygate/worktree-briefs/pane-landing-brief.md` (your build brief)
- `/Users/saneel/projects/paygate/docs/` (list the directory to see existing files)
- `/Users/saneel/projects/paygate/docs/quickstart.html` (existing page — add Plausible script to it)
- `/Users/saneel/projects/paygate/SPEC.md` (section 4.1 for 402 response format — code examples must match)
- `/Users/saneel/projects/paygate/worktree-briefs/pane-demo-apis-brief.md` (API endpoints, prices, and example requests/responses for the "Available APIs" section)

### Step 2: Build everything

1. Create `docs/style.css` — the shared stylesheet with all styles from the brief
2. Create `docs/index.html` — the main landing page with all 8 sections
3. Update `docs/quickstart.html` — add Plausible script and optional link to style.css

Key details:
- Pure HTML + CSS, no JavaScript (besides Plausible analytics script)
- System fonts only (no external font loading)
- All code examples must use accurate API shapes from the demo brief
- Use `https://demo-paygate.fly.dev` as placeholder URL throughout
- Dark hero, light content sections, dark "Add Your API" section
- Terminal-style code boxes with the 3-dot title bar
- 4 API cards in a responsive grid
- Mobile-responsive (test at 375px, 768px, 1200px widths)

### Step 3: Verify

Open the page in a browser to check:
```bash
open /Users/saneel/projects/paygate/docs/index.html
```

Verify:
- Page renders correctly
- All sections present and readable
- Code blocks are properly formatted
- Links work (GitHub, tempo.xyz, quickstart.html)
- Plausible script tag present in both HTML files
- Page looks reasonable on a narrow viewport (resize browser window)

### Step 4: Check page weight

```bash
wc -c docs/index.html docs/style.css
```

Total should be under 100KB.

### Step 5: Commit

```
feat(docs): add product landing page with Plausible analytics

Landing page at docs/index.html with hero, how-it-works, live demo,
API catalog, create-paygate instructions, SDK example, and footer.
Adds shared stylesheet and Plausible analytics to all HTML pages.
```
