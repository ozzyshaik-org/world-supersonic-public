# Demo script: single paid `/v1/mev/analytics` investigation

Status: **draft script**. Not a recorded demo. Show a step only if it works; otherwise show an honest pending /
`MISSING` / `NOT_YET_RUN` state. Do not simulate live payment success.

Frozen narrative: an agent pays once for a derived MEV analytics lead, then opens the **same** World Chain `tx_hash` in
SGIP. The Arc payment / Gateway transfer ID stays **separate** from that investigated `tx_hash`.

Do **not** demo `/v1/balance` as a wallet balance.

## Artifacts (owner-supplied when ready)

| Artifact | Status |
|---|---|
| Arc Studio project URL (`studio.arc.io`) | `MISSING` |
| Public demo URL | `MISSING` |
| Live Arc Testnet / Circle Gateway payment | `NOT_YET_RUN` |
| Demo / submission video | `MISSING` |
| Experimental seller-engine at `f79de2f` | `built-unit-fake` / `contract-real-local` (see [EVIDENCE.md](../EVIDENCE.md)) |
| Real HTTP adapter | draft PR #25 `b03d41e`: `local-http-integration` + unit-fake Gateway (137/137); **not** live Arc |

## Timed script (target ≤ 4 minutes once live pieces exist)

| Time | Step | What to show | Status label |
|---|---|---|---|
| 0:00–0:20 | Continuity boundary | Pre-existing World Chain index versus event extension; start tag `ethglobal-mumbai-2026-start`; Corrected Variant B | ready (docs) |
| 0:20–0:40 | Architecture | [ARCHITECTURE_SLICE.md](ARCHITECTURE_SLICE.md) diagram; call out each box status | draft diagram ready |
| 0:40–0:55 | Studio origin | Real Studio project that the seller starts from | `MISSING` |
| 0:55–1:40 | Unpaid → 402 | `GET /v1/mev/analytics` without payment returns HTTP 402 with machine-readable requirements | engine unit-fake + **local-http-integration COVERED** at `b03d41e` (FAKE Gateway); live Arc still `NOT_YET_RUN` |
| 1:40–2:20 | Pay → settle → fulfill | Client signs; seller validates/reserves; `settle()`; only after acceptance call Go API; return stored response/receipt | unit-fake + local-http-integration settle-before-upstream/2xx at `b03d41e`; **live Gateway**: `NOT_YET_RUN` |
| 2:20–2:50 | Same-binding replay | Retry same nonce+binding → stored response, no second charge | unit-fake + local-http-integration COVERED at `b03d41e`; live Arc still `NOT_YET_RUN` |
| 2:50–3:20 | Investigate | Open SGIP on the response's World Chain `tx_hash` (not the Arc payment id) | SGIP pre-existing; paid E2E `NOT_YET_RUN` |
| 3:20–3:40 | Limits | No live batching/`completed` claim unless observed; IDKit/Agent Pass/ENS deferred; link [EVIDENCE.md](../EVIDENCE.md) and [AI_USAGE.md](../AI_USAGE.md) | honest limits |

## Deferred (do not imply working)

- IDKit self-service free-tier mint
- World ID Agent Pass approval checkpoint
- ENS name inputs
- pstack formal verification

## Failure / honesty rules

- Never convert FAKE Gateway unit results into live Arc claims.
- Never mark ARC live rows passed unless the complete stated procedure in [EVIDENCE.md](../EVIDENCE.md) was run.
- Never claim "every rule" without the corresponding executable test.
- If Studio URL, public demo, live payment, or video is absent, say `MISSING` on camera / in the submission form.
