# Evidence register

Status: template, pre-start documentation. Nothing in this register has been run. Every event row is `NOT_YET_RUN` or
`PLANNED`. The Commit, Actual and Proof columns stay empty until a real run produces a real result. Do not enter
transaction hashes, URLs or results that were not produced by a recorded run.

> Authorization, fulfillment, batching and settlement require separate evidence. Gateway balance/transfer/status evidence must not be replaced by an unrelated recipient-wallet balance check.

Status values:

- `NOT_YET_RUN`: the claim has a defined procedure, and the procedure has not been run.
- `PLANNED`: the claim is intended but its procedure is not yet defined in detail.

Specifications: [Arc x402](docs/hackathon/01-x402-arc-gateway.md), [World ID IDKit](docs/hackathon/02-world-id-gating.md),
[World ID Agent Pass](docs/hackathon/04-agent-pass.md), [ENS](docs/hackathon/03-ens-integration.md). Feature work is event work and none of it is implemented yet.

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

## World ID IDKit (priority 2, core)

| Claim | Feature | Status | Commit | Command/procedure | Expected | Actual | Proof | Limitations |
|---|---|---|---|---|---|---|---|---|
| A self-service key is minted only after a World ID proof | World ID mint | NOT_YET_RUN | | Mint flow with a valid proof | One active self-service free-tier key per action-scoped nullifier | | | Operator-minted keys are a separate documented exception |
| A duplicate nullifier is refused | World ID duplicate nullifier | NOT_YET_RUN | | Second mint with the same action-scoped nullifier | Second self-service key refused | | | |
| Lost-key recovery with a fresh proof replaces the key | World ID recovery | NOT_YET_RUN | | Recovery with a fresh valid proof | Old key revoked, replacement issued, no new nullifier row | | | |
| A revoked key is refused on `/v1` within 5 seconds | World ID revocation `/v1` | NOT_YET_RUN | | Revoke a key and call `/v1` with it | Refused within 5 s | | | Tested upper-bound target under normal operating conditions, not a production SLA |
| A revoked key's open `/ws` session is closed within 10 seconds | World ID revocation `/ws` | NOT_YET_RUN | | Revoke a key with an open `/ws` session | Session closed within 10 s | | | Tested upper-bound target under normal operating conditions, not a production SLA |
| Edge-port bypass is closed | World ID edge-port bypass | PLANNED | | External probe from outside the host against `:8081`, `:8090` and `:8091` | Direct access refused, closed or key-checked | | | |
| Key checks fail closed | World ID fail-closed | PLANNED | | Make the key store unavailable and send requests | Requests refused, not allowed through | | | The README discloses that `cmd/api` fails open today |

## World ID Agent Pass (priority 3, targeted)

Separate from the IDKit rows above: IDKit free-tier uniqueness and Agent Pass action approval are separate integrations, and evidence for one is never used for the other. Agent Pass approval is not Arc/x402 payment, so a paid operation also needs its own Arc evidence. Do not enter ID tokens, authorization codes, secrets, request bodies or sensitive query content in any column; use operation IDs and truncated fingerprints only.

| Claim | Feature | Status | Commit | Command/procedure | Expected | Actual | Proof | Limitations |
|---|---|---|---|---|---|---|---|---|
| An agent workflow is paused and the exact operation is stored | Agent Pass paused workflow | NOT_YET_RUN | | Agent proposes one paid or elevated request; inspect the stored pending operation and try to execute before any approval | Operation stored with account, fingerprint, assurance policy and expiry; protected action not executed | | | |
| Fresh authentication is requested and validated by the backend | Agent Pass fresh authentication | NOT_YET_RUN | | Start the attempt with `max_age=0`; complete World ID authentication; the backend validates state, one-time code redemption, ID token, nonce, issuer, audience, expiry and account/session binding | Result accepted only after all checks; `auth_time` and `acr` meet the saved policy and `amr` equals the saved profile as an unordered set (`["pop"]` baseline; `["pop", "face"]` when Face Auth is required) | | | Mock proofs and fake identities are noted on the prizes page; they do not show real human assurance |
| A valid identity alone does not authorize the action | Agent Pass assurance policy | NOT_YET_RUN | | Authenticate with a wrong `(iss, sub)` for a linked account, with a result below the `acr` or `auth_time` policy, and with the passkey-only profile `amr` `["pop", "user", "mfa"]` (which contains `pop`) and another non-matching `amr` set, under the baseline Orb policy and the Face Auth policy | All rejected; no approval can be recorded; `["pop"]` (baseline) and `["pop", "face"]` (Face Auth) with the correct `acr` are accepted regardless of array order | | | |
| A separate human approval of the exact action is recorded in our application | Agent Pass action approval | NOT_YET_RUN | | After validation, show the approval view (agent/account, method and route, canonical query summary, price or access consequence, fingerprint, expiry) and approve | Approval row bound to the account, operation and displayed fingerprint | | | The World ID issuer authenticates the human; it does not approve the action |
| The approved operation executes exactly once | Agent Pass single execution | NOT_YET_RUN | | Resume the approved operation; recheck, consume the approval and reserve the operation in one transaction | One execution; approval marked used in the same transaction; a paid operation also completes its separate Arc/x402 payment | | | Approval is not payment, fulfillment or settlement; per-request onchain settlement is not claimed |
| A denied operation stays blocked | Agent Pass denial | NOT_YET_RUN | | Deny the displayed action, then try to resume | Protected action not executed | | | |
| Cancelled and expired operations stay blocked | Agent Pass cancel and expiry | NOT_YET_RUN | | Cancel one operation and let another expire (including after approval and before execution), then try to resume each | Protected action not executed; expiry not extended by status checks or retries | | | |
| A backend-validation failure fails closed | Agent Pass validation failure | NOT_YET_RUN | | Inject a wrong state, wrong nonce, wrong audience and an issuer or token endpoint failure, one at a time | Attempt failed; no approval; nothing executed | | | |
| A changed request needs a new approval | Agent Pass changed fingerprint | NOT_YET_RUN | | Change the route, query, price or account after approval and resume | Refused; new proposal and new approval required | | | |
| Replay and double execution are prevented | Agent Pass replay | NOT_YET_RUN | | Replay the callback, code, approval and resume; send two concurrent resumes | At most one execution; others refused | | | |
| An uncertain mutating outcome is not blindly retried | Agent Pass stable operation ID | NOT_YET_RUN | | Simulate a lost response after execution starts | Outcome looked up by operation ID; no second approval consumed and no repeated authentication | | | |
| No World ID token is forwarded and no secret reaches logs or evidence | Agent Pass log and token hygiene | NOT_YET_RUN | | Review downstream requests, logs and this register for ID tokens, codes, secrets, bodies and sensitive query content | None present | | | |
| The approval journey reuses the event web frontend | Agent Pass frontend | PLANNED | | Show the approval views inside the event web frontend | No separate unrelated UI | | | |
| World Agent Pass integration debrief is written from real experience | Agent Pass debrief | PLANNED | | Complete the debrief template in the spec after the integration is attempted | Use case, time to first success, friction, missing capability or documentation, one high-impact improvement, what we changed | | | Template only until the integration is attempted |

## ENS (priority 4, conditional)

| Claim | Feature | Status | Commit | Command/procedure | Expected | Actual | Proof | Limitations |
|---|---|---|---|---|---|---|---|---|
| ENS names are accepted for `/v1/balance` and `/v1/transactions` | ENS name inputs | PLANNED | | Attempted only after the Arc, World ID IDKit and World ID Agent Pass flows work end to end with their evidence complete | Names resolve to addresses for the two routes | | | Fourth priority and conditional |
| No ENSv2 prize claim from ordinary `.eth` resolution | ENS prize eligibility | PLANNED | | Review the final integration against the ENS prize criteria | Prize claimed only if ENSv2 on Ethereum mainnet is used in a functional, central way | | | Ordinary `.eth` resolution alone does not support an ENSv2 prize claim |
