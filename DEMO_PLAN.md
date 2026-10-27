# Demo plan

Status: plan only, pre-start documentation. This is not an existing demo. None of the event flows below is implemented,
and each step will be shown only if it works. A step that does not work will be shown as an honest pending state, not simulated.

Narrative: Supersonic makes World Chain data accessible to both autonomous agents and verified humans: agents pay per query, humans receive protected free-tier access, and a human approves an agent's paid or elevated request with fresh authentication.

## Steps

1. **Pre-existing index and measurement date.** Show the already-running World Chain index and state the date its figures were measured (the README holds the dated 2026-10-24 figures). This is baseline work.
2. **Paid request.** A paid request returns HTTP 402, the client signs the Arc authorization, and the seller validates it, submits it to Circle Gateway (`settle()`), and only after Gateway accepts it returns a real answer with block context and a receipt. Show acceptance and fulfillment as two separate events.
3. **Same-request retry.** Retry the same request and show that no additional charge occurs.
4. **Conflicting nonce reuse.** Reuse a nonce for a different request and show HTTP 409.
5. **Gateway batching and settlement.** Show the transfer's real Gateway status by nonce: accepted, then batched, then `completed`, or an honest pending state if Gateway has not completed it. The seller does not create or submit batches, and a successful response is not onchain settlement.
6. **World ID IDKit.** Mint a key with a World ID proof, show the duplicate refusal, then show revocation evidence.
7. **Agent Pass: workflow paused.** An agent proposes one paid or elevated request; show that the backend paused it and stored the exact operation, and that it has not executed.
8. **Agent Pass: fresh authentication.** The human completes fresh World ID authentication (`max_age=0`) in the event web frontend; show the backend validating the result against the saved policy.
9. **Agent Pass: separate human approval.** Show the approval view (agent/account, method and route, query summary, payment amount, network, asset and `payTo` or the access consequence, fingerprint, expiry) and the human approving the exact action in our application. This is not the Arc payment.
10. **Agent Pass: one successful execution.** Send the paid protected request for the approved operation and show the reservation committed before `settle()`, then the operation execute exactly once. A paid request has its separate Arc/x402 payment, and a payment sent without a valid reservation is refused before it is submitted.
11. **Agent Pass: blocked paths.** Show a denied, a cancelled and an expired action each remaining blocked, and show that cancellation is refused once the execution reservation has committed.
12. **Agent Pass: changed request and replay.** Show a changed request needing a new approval, and a replay or double resume producing no second execution.
13. **Agent Pass: integration debrief.** Show the integration debrief, or an honest pending state if it is not written.
14. **ENS.** Only after the core flows (steps 2 to 13) are complete.
15. **Event diff, AI usage and limitations.** Show the event diff since `ethglobal-mumbai-2026-start`, [AI_USAGE.md](AI_USAGE.md) and the known limitations.

Evidence for each step is recorded in [EVIDENCE.md](EVIDENCE.md). Specifications: [Arc x402](docs/hackathon/01-x402-arc-gateway.md),
[World ID IDKit](docs/hackathon/02-world-id-gating.md), [World ID Agent Pass](docs/hackathon/04-agent-pass.md), [ENS](docs/hackathon/03-ens-integration.md).
