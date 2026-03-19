You are running /review on the PayGate project — a pre-landing code review of the full codebase.

This is a greenfield project on the main branch at ~/projects/paygate. There is no remote/PR yet — review the entire codebase as if it were a single PR about to land.

Read these files for context:
- SPEC.md (the authoritative specification)
- ENG-REVIEW.md (engineering review decisions)
- DESIGN-REVIEW.md (CLI output specifications)
- docs/designs/error-rescue-registry.md (error handling spec)
- docs/designs/security-threat-model.md (security requirements)
- .gstack/qa-reports/qa-report-paygate-2026-03-19.md (QA findings — 8 bugs already fixed)

Then review ALL source code:
- crates/paygate-common/src/ (all files)
- crates/paygate-gateway/src/ (all files — main.rs, config.rs, db.rs, server.rs, verifier.rs, mpp.rs, proxy.rs, rate_limit.rs, webhook.rs, metrics.rs, admin.rs)
- sdk/src/ (all files)
- sdk/tests/ (all files)
- contracts/src/ (all files)
- contracts/test/ (all files)
- schema.sql

Review for:
1. SQL safety — any injection vectors? Parameterized queries used everywhere?
2. Trust boundary violations — does payment data flow correctly? Can untrusted input reach trusted operations?
3. Concurrency issues — race conditions in SQLite writer? Token bucket thread safety?
4. Error handling completeness — are there any unwrap() calls that could panic in production?
5. Security — constant-time comparisons where needed? Private key handling? Header sanitization complete?
6. Architectural coherence — do the 4 merged branches (verifier, cli, ts-sdk, contracts) integrate cleanly?
7. Correctness — does the code actually do what the spec says? Any subtle logic bugs?
8. Dead code — anything that should be cleaned up before shipping?

Write your review report to .gstack/qa-reports/code-review-2026-03-19.md. Flag anything that should be fixed before shipping v0.1.0.
