# Hackathon log (ETHGlobal Mumbai, 5-7 Nov 2026)

Official hacking start: **2026-11-05 21:00:00 IST**.

Annotated start tag: `ethglobal-mumbai-2026-start`
- Tag object: `7993c080fa47d3f15e584795fcb4bd59c0250012`
- Peeled commit: `cb39dbb3d2dbf217949ef279c7e526c543c0b7ea`
- This tag must never be moved or backdated.

**Corrected Variant B** applies (see [CONTINUITY.md](CONTINUITY.md)):
- PR #21's seven branch commits (`c65f36f` through `e4b6346`) were made before hacking began and remain pre-existing work.
- Only the merge commit `cb39dbb` is event-period administrative/documentation work from PR #21.
- No part of PR #21 is claimed as an event-built feature.
- Every other commit reachable from the start tag is pre-existing work.
- Commits after the start tag are event work, subject to their actual content and timestamps.

Rows below record event-period work. Pre-existing work remains tag `baseline-pre-ethglobal-mumbai` and the pre-start commits listed in [CONTINUITY.md](CONTINUITY.md).

Rows for draft PR [#23](https://github.com/ozzyshaik-org/world-supersonic-public/pull/23) record **experimental seller-engine** checkpoint commits on branch `feat/arc-x402-seller`. They are event-built after the start tag and are **not** yet on `main`. Do not treat them as a live HTTP seller, Arc Studio UI, or live Circle Gateway integration. Package-local checkpoint documentation lives under `x402-seller/` on that PR and is not edited by this docs PR.

| Time (IST) | Feature | What changed | Commit |
|---|---|---|---|
| 2026-11-05 21:17 | (admin/docs) | Merge of PR #21 (documentation and website wording only). Event-period administrative/documentation under Corrected Variant B; not an event-built feature. | `cb39dbb` |
| 2026-11-06 02:17 | Arc x402 (experimental seller-engine) | Scaffold TypeScript `x402-seller/` package (event-built after start tag). Draft PR #23; not merged. | `5dee2bf` |
| 2026-11-06 02:17 | Arc x402 (experimental seller-engine) | auth_key, fingerprint v1, types, price table. Draft PR #23. | `b983c63` |
| 2026-11-06 02:17 | Arc x402 (experimental seller-engine) | Gateway interface, FAKE clients, memory ledger. Draft PR #23. | `6910138` |
| 2026-11-06 02:17 | Arc x402 (experimental seller-engine) | Settle-before-fulfill seller engine. Draft PR #23. | `572f41e` |
| 2026-11-06 02:17 | Arc x402 (experimental seller-engine) | FAKE Gateway unit tests for state-machine rules. Draft PR #23. | `6778545` |
| 2026-11-06 08:16 | Arc x402 (experimental seller-engine) | Canonical x402 v2 `payment.accepted` + strict validation. Draft PR #23. | `283ee59` |
| 2026-11-06 08:16 | Arc x402 (experimental seller-engine) | Settle classification, fail-closed reconcile, lease fencing. Draft PR #23. | `0d58eab` |
| 2026-11-06 08:16 | Arc x402 (experimental seller-engine) | CI job, engines, scoped ignores, Circle import contract. Draft PR #23. | `684b0da` |
| 2026-11-06 08:20 | Arc x402 (experimental seller-engine) | Waiters poll for lease holder; tighten takeover tests. Draft PR #23. | `dc761fd` |
| 2026-11-06 12:52 | Arc x402 (experimental seller-engine) | Atomic retryable/budget CAS under original lease (race fix). Head of draft PR #23: `f79de2f`. Local/unit evidence only: 116/116 tests, five race tests, ledger-level pre-fix oracle on `dc761fd`; CI run 42180463805 success. **Not** live Arc/Circle. Pre-fix full engine-level 2xx rejection was not independently reproduced. | `f79de2f` |
