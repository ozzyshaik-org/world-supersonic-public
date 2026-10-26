# Evidence register

Status: template, pre-start documentation. Nothing in this register has been run. Every event row is `NOT_YET_RUN` or
`PLANNED`. The Commit, Actual and Proof columns stay empty until a real run produces a real result. Do not enter
transaction hashes, URLs or results that were not produced by a recorded run.

> Authorization, fulfillment, batching and settlement require separate evidence. Gateway balance/transfer/status evidence must not be replaced by an unrelated recipient-wallet balance check.

Status values:

- `NOT_YET_RUN`: the claim has a defined procedure, and the procedure has not been run.
- `PLANNED`: the claim is intended but its procedure is not yet defined in detail.

Specifications: [Arc x402](docs/hackathon/01-x402-arc-gateway.md), [World ID](docs/hackathon/02-world-id-gating.md),
[ENS](docs/hackathon/03-ens-integration.md). Feature work is event work and none of it is implemented yet.

## Arc: x402 with Circle Gateway (priority 1, core)

| Claim | Feature | Status | Commit | Command/procedure | Expected | Actual | Proof | Limitations |
|---|---|---|---|---|---|---|---|---|
| A paid `/v1` route without payment returns the 402 challenge | Arc x402 402 challenge | NOT_YET_RUN | | Request a paid route with no payment header | HTTP 402 with machine-readable payment requirements (price, asset, network, `payTo`) | | | |
| A signed authorization is verified and accepted | Arc x402 authorization | NOT_YET_RUN | | Retry with a signed payment authorization | Seller verifies the authorization with Circle Gateway and records it in the ledger | | | Authorization is not fulfillment or settlement |
| An accepted request is fulfilled | Arc x402 fulfillment | NOT_YET_RUN | | Same request after authorization | Real answer from the Go API with block context, response and receipt stored | | | Fulfillment is not settlement |
| Same-request retry is not charged twice | Arc x402 idempotent retry | NOT_YET_RUN | | Resend the same request with the same nonce and fingerprint | Stored response and receipt returned; no second charge | | | |
| Nonce reuse with a different fingerprint is rejected | Arc x402 nonce conflict | NOT_YET_RUN | | Reuse a nonce with a different request | HTTP 409; no upstream call, no state change, no charge | | | |
| Fulfilled authorizations are batched | Arc x402 batching | NOT_YET_RUN | | Run the settlement job over fulfilled authorizations | Fulfilled authorizations added to a batch; only fulfilled ones are included | | | Batching is not settlement |
| Batches are settled through Circle Gateway on Arc | Arc x402 Gateway settlement | NOT_YET_RUN | | Gateway balance, transfer and status evidence for the batch | Batch confirmed on Arc through Circle Gateway; at most one settlement per authorization | | | Per-request onchain settlement is not claimed. Not replaceable by a recipient-wallet balance check |
| Paid routes cannot be bypassed | Arc paid-route enforcement | PLANNED | | External probe from outside the host against `:8081` and other proxy paths | Direct access to a paid route is refused or closed, or the route is a documented free tier | | | |

## World ID (priority 2, core)

| Claim | Feature | Status | Commit | Command/procedure | Expected | Actual | Proof | Limitations |
|---|---|---|---|---|---|---|---|---|
| A self-service key is minted only after a World ID proof | World ID mint | NOT_YET_RUN | | Mint flow with a valid proof | One active self-service free-tier key per action-scoped nullifier | | | Operator-minted keys are a separate documented exception |
| A duplicate nullifier is refused | World ID duplicate nullifier | NOT_YET_RUN | | Second mint with the same action-scoped nullifier | Second self-service key refused | | | |
| Lost-key recovery with a fresh proof replaces the key | World ID recovery | NOT_YET_RUN | | Recovery with a fresh valid proof | Old key revoked, replacement issued, no new nullifier row | | | |
| A revoked key is refused on `/v1` within 5 seconds | World ID revocation `/v1` | NOT_YET_RUN | | Revoke a key and call `/v1` with it | Refused within 5 s | | | Tested upper-bound target under normal operating conditions, not a production SLA |
| A revoked key's open `/ws` session is closed within 10 seconds | World ID revocation `/ws` | NOT_YET_RUN | | Revoke a key with an open `/ws` session | Session closed within 10 s | | | Tested upper-bound target under normal operating conditions, not a production SLA |
| Edge-port bypass is closed | World ID edge-port bypass | PLANNED | | External probe from outside the host against `:8081`, `:8090` and `:8091` | Direct access refused, closed or key-checked | | | |
| Key checks fail closed | World ID fail-closed | PLANNED | | Make the key store unavailable and send requests | Requests refused, not allowed through | | | The README discloses that `cmd/api` fails open today |

## ENS (priority 3, conditional)

| Claim | Feature | Status | Commit | Command/procedure | Expected | Actual | Proof | Limitations |
|---|---|---|---|---|---|---|---|---|
| ENS names are accepted for `/v1/balance` and `/v1/transactions` | ENS name inputs | PLANNED | | Attempted only after the Arc and World ID flows work end to end with their evidence complete | Names resolve to addresses for the two routes | | | Third priority and conditional |
| No ENSv2 prize claim from ordinary `.eth` resolution | ENS prize eligibility | PLANNED | | Review the final integration against the ENS prize criteria | Prize claimed only if ENSv2 on Ethereum mainnet is used in a functional, central way | | | Ordinary `.eth` resolution alone does not support an ENSv2 prize claim |
