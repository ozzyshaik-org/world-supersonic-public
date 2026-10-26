# Demo plan

Status: plan only, pre-start documentation. This is not an existing demo. None of the event flows below is implemented,
and each step will be shown only if it works. A step that does not work will be shown as an honest pending state, not simulated.

Narrative: Supersonic makes World Chain data accessible to both autonomous agents and verified humans: agents pay per query, while humans receive protected free-tier access.

## Steps

1. **Pre-existing index and measurement date.** Show the already-running World Chain index and state the date its figures were measured (the README holds the dated 2026-10-24 figures). This is baseline work.
2. **Paid request.** A paid request returns HTTP 402, the client makes the Arc authorization, and the seller returns a real answer with block context and a receipt.
3. **Same-request retry.** Retry the same request and show that no additional charge occurs.
4. **Conflicting nonce reuse.** Reuse a nonce for a different request and show HTTP 409.
5. **Settlement.** Show real Gateway/batch settlement evidence, or an honest pending state if settlement has not completed.
6. **World ID.** Mint a key with a World ID proof, show the duplicate refusal, then show revocation evidence.
7. **ENS.** Only after the core flows (steps 2 to 6) are complete.
8. **Event diff, AI usage and limitations.** Show the event diff since `ethglobal-mumbai-2026-start`, [AI_USAGE.md](AI_USAGE.md) and the known limitations.

Evidence for each step is recorded in [EVIDENCE.md](EVIDENCE.md). Specifications: [Arc x402](docs/hackathon/01-x402-arc-gateway.md),
[World ID](docs/hackathon/02-world-id-gating.md), [ENS](docs/hackathon/03-ens-integration.md).
