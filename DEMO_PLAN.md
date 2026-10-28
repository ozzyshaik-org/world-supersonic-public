# Demo plan

Status: plan only, pre-start documentation. This is not an existing demo. None of the event flows below is implemented,
and each step will be shown only if it works. A step that does not work will be shown as an honest pending state, not simulated.

Narrative: **an agent pays once for an execution-level lead, follows its transaction hash into Supersonic's investigation
graph, and leaves with an inspectable response and receipt.** World ID IDKit protects the human free tier; Agent Pass adds
a fresh human checkpoint and a separate application approval for one paid or elevated agent request. These integrations
support the investigation product; they are not presented as a sponsor-logo sequence.

The investigation and public-fixture contract is in [spec 05](docs/hackathon/05-investigation-evidence.md). A derived MEV
row remains a candidate/finding, not proof of intent, guaranteed classification or realized profit. Live chain data, the
synthetic/sanitized fixture, real partner runs and pending states are labelled separately.

## Main 2–4 minute video

This is a time budget, not a claim that any step already works. Cut an unfinished flow and its prize claim rather than
filling time with a mock. Show the longer failure matrix in the live judging appendix below, not by rushing the main story.

| Target time | Segment | What must be visible |
|---|---|---|
| 0:00–0:15 | One claim and continuity boundary | The investigation sentence above; pre-existing index versus event extension; measured production figures keep their date. |
| 0:15–0:35 | Arc Studio and architecture | The real Studio project on `studio.arc.io`, the Studio-built frontend and seller backend, and a compact client → seller → Circle Gateway → Go API/SGIP diagram. No placeholder URL. |
| 0:35–1:20 | Paid investigation | An existing `/v1/mev/analytics` request returns 402; the client signs; the seller validates, reserves and calls `settle()`; only after Gateway accepts does it fulfill. The response's `tx_hash` opens the SGIP graph for that same transaction, with block context and receipt visible. Acceptance and fulfillment are separate events. |
| 1:20–1:40 | Retry and Gateway truth | After fulfillment, the same authorization returns the stored response with no second charge; a different request reusing the nonce gets 409. A status panel shows only observed Gateway states: `received` is not batching, `batched`/`confirmed` is direct batching evidence, and `completed` is final settlement. No waiting is left in the video. |
| 1:40–2:05 | IDKit trust moment | Mint one self-service free-tier key with a supported World ID credential verified by the backend; show the duplicate refusal and revoked-key refusal. Explain in one sentence why that credential is the minimum sufficient assurance. |
| 2:05–2:50 | Agent Pass checkpoint | Pause one exact paid or elevated operation; complete fresh authentication (`max_age=0`); separately approve the displayed action in our application; reserve before `settle()` and execute once. Show one denied/cancelled/changed/replayed path remaining blocked. Agent Pass approval is not Arc payment. |
| 2:50–3:20 | Reproduce and inspect | Run the one-command public-fixture verification and its negative control, then show the event diff, code pointers and AI-use entry point. State that the fixture is synthetic/sanitized and proves only the documented REST/SGIP contract. |
| 3:20–3:40 | Limits and submission pointers | Real demo/repository/Studio links, integration debriefs and material limitations. ENS appears only if actually completed; an unfinished feature is not implied to work. |

If the full working flow cannot fit honestly, the priority is the paid investigation, one complete World flow and the
continuity/reproducibility evidence. Do not claim the other World track merely because it appears in this plan.

## Live judging and evidence appendix

Run these as separate inspectable checks, with [EVIDENCE.md](EVIDENCE.md) recording only real results:

1. **Pre-existing index and measurement date.** Show the already-running World Chain index and state that its README figures
   were measured on 2026-10-24. This is baseline work, not an event result.
2. **Arc acceptance and fulfillment.** Show the ledger reservation, `settle()` success, upstream fulfillment and stored
   response/receipt as separate observations. A successful response is not onchain settlement.
3. **Arc retry and conflict.** Complete fulfillment before demonstrating the stored-response retry. Then reuse the nonce for
   a different fingerprint and show HTTP 409 with no new Gateway call, upstream call or charge.
4. **Gateway observation.** Reconcile by nonce with the network explicit. Batching is claimed only when `batched` or
   `confirmed` is observed; final settlement only at `completed`. If only `completed` is observed, say that batching was not
   directly observed. The seller creates and submits no batch.
5. **Arc failure paths.** Show invalid/definitely rejected payment, accepted-but-unfulfilled retry, uncertain submission,
   late matching transfer and post-acceptance upstream failure only where the corresponding real test or clearly labelled
   integration test has actually run. Empty transfer lookups are not proof of non-acceptance; attempted uncertain rows are
   never auto-voided.
6. **IDKit alternative paths.** Show cancellation/denial, duplicate and concurrent mint, recovery, rotation, revocation on
   `/v1` and `/ws`, fail-closed key checks and closure of direct-port bypasses. Operator-minted keys never stand in for the
   self-service human flow.
7. **Agent Pass approval binding.** Show the exact stored operation and the approval view (agent/account, method/route,
   query summary, payment network/asset/amount/`payTo` or access consequence, fingerprint and expiry). Authentication and
   application approval remain separate.
8. **Agent Pass blocked paths.** Show denied, cancelled, expired, mismatched, changed-request, replayed and double-resume
   cases leaving the action unexecuted or producing no second execution. Cancellation is refused after reservation commits.
9. **Combined Arc × Agent Pass outcomes.** Show one reservation/approval/payment binding, no direct payment without a valid
   reservation, one logical fulfillment, and honest `paid_unfulfilled` or `fulfilled_but_unpaid` handling where applicable.
10. **ENS, fourth priority.** Only after Arc, IDKit and Agent Pass are complete and evidenced. Show mainnet resolution on the
    two committed routes, no-charge preflight failure and pinned-address fulfillment. Ordinary `.eth` resolution is not an
    ENSv2 prize claim.
11. **Evaluator reproduction.** From a clean checkout, run the public fixture command and negative control. Show the manifest
    and expected assertions; do not present fixture output as live chain data or as partner-run evidence.
12. **Continuity, AI and limitations.** Show the diff since `ethglobal-mumbai-2026-start`, [AI_USAGE.md](AI_USAGE.md), the
    integration debriefs and known limitations. No start tag or event result exists merely because it is listed here.

Evidence for each claimed step is recorded in [EVIDENCE.md](EVIDENCE.md). Specifications:
[Arc x402](docs/hackathon/01-x402-arc-gateway.md), [World ID IDKit](docs/hackathon/02-world-id-gating.md),
[World ID Agent Pass](docs/hackathon/04-agent-pass.md), [ENS](docs/hackathon/03-ens-integration.md) and
[investigation/evidence](docs/hackathon/05-investigation-evidence.md).
