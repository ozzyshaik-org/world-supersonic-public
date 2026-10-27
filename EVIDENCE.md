# Evidence register

Status: template, pre-start documentation. Nothing in this register has been run. Every event row is `NOT_YET_RUN` or
`PLANNED`. The Commit, Actual and Proof columns stay empty until a real run produces a real result. Do not enter
transaction hashes, URLs or results that were not produced by a recorded run.

> Authorization, fulfillment, batching and settlement require separate evidence. Gateway balance/transfer/status evidence must not be replaced by an unrelated recipient-wallet balance check.

Status values:

- `NOT_YET_RUN`: the claim has a defined procedure, and the procedure has not been run.
- `PLANNED`: the claim is intended but its procedure is not yet defined in detail.

Row IDs: `ARC-nn` (Arc), `IDK-nn` (World ID IDKit), `AP-nn` (Agent Pass), `ENS-nn` (ENS) and `X-nn` (cross-feature). The "Done means" column names the spec item each row traces to, written `<spec>-DM<n>` (for example `01-DM5` is item 5 of spec 01), and the last section lists every item with its rows. Rows in the single-feature tables may cover only part of an item; the procedure column says what is run.

Specifications: [Arc x402](docs/hackathon/01-x402-arc-gateway.md), [World ID IDKit](docs/hackathon/02-world-id-gating.md),
[World ID Agent Pass](docs/hackathon/04-agent-pass.md), [ENS](docs/hackathon/03-ens-integration.md). Feature work is event work and none of it is implemented yet.

## Arc: x402 with Circle Gateway (priority 1, core)

| ID | Claim | Feature | Done means | Status | Commit | Command/procedure | Expected | Actual | Proof | Limitations |
|---|---|---|---|---|---|---|---|---|---|---|
| ARC-01 | A paid `/v1` route without payment returns the 402 challenge | Arc x402 402 challenge | 01-DM1 | NOT_YET_RUN | | Request a paid route with no payment header | HTTP 402 with machine-readable payment requirements (price, asset, network, `payTo`) | | |  |
| ARC-02 | A signed authorization is verified and accepted | Arc x402 authorization | 01-DM2 | NOT_YET_RUN | | Retry with a signed payment authorization | Seller verifies the authorization with Circle Gateway and records it in the ledger | | | Authorization is not fulfillment or settlement |
| ARC-03 | An accepted request is fulfilled | Arc x402 fulfillment | 01-DM2 | NOT_YET_RUN | | Same request after authorization | Real answer from the Go API with block context, response and receipt stored | | | Fulfillment is not settlement |
| ARC-04 | Same-request retry is not charged twice | Arc x402 idempotent retry | 01-DM5 | NOT_YET_RUN | | Resend the same request with the same nonce and fingerprint | Stored response and receipt returned in the fulfilled, batched and settled states; no upstream call, no Gateway call, no second fulfillment or settlement | | |  |
| ARC-05 | Nonce reuse with a different fingerprint is rejected | Arc x402 nonce conflict | 01-DM5 | NOT_YET_RUN | | Reuse a nonce with a different fingerprint, payer, price, asset, network or `payTo`, one case per field | HTTP 409 `authorization_binding_mismatch`; no upstream call, no Gateway call, no state change, no charge | | |  |
| ARC-06 | Fulfilled authorizations are batched | Arc x402 batching | 01-DM2, 01-DM11 | NOT_YET_RUN | | Run the settlement job over fulfilled authorizations | Fulfilled authorizations added to a batch; only fulfilled ones are included | | | Batching is not settlement |
| ARC-07 | Batches are settled through Circle Gateway on Arc | Arc x402 Gateway settlement | 01-DM2 | NOT_YET_RUN | | Gateway balance, transfer and status evidence for the batch | Batch confirmed on Arc through Circle Gateway; at most one settlement per authorization | | | Per-request onchain settlement is not claimed. Not replaceable by a recipient-wallet balance check |
| ARC-08 | Paid routes cannot be bypassed | Arc paid-route enforcement | 01-DM6 | PLANNED | | External probe from outside the host against `:8081` and other proxy paths | Direct access to a paid route is refused or closed, or the route is a documented free tier | | |  |
| ARC-09 | An invalid authorization is not fulfilled and does not reach the Go API | Arc x402 invalid authorization | 01-DM4 | NOT_YET_RUN | | Send a malformed, wrongly signed and expired authorization | Each refused; no upstream call; ledger records no fulfillment | | |  |
| ARC-10 | The Arc Studio project is the starting point of the seller service | Arc Studio origin | 01-DM3 | PLANNED | | Show the Studio project and the seller service derived from it | Project exists and the seller service starts from it | | | Studio URL is to be rechecked (see the submission checklist) |
| ARC-11 | The Studio-built web UI performs the full flow against the seller service | Arc Studio frontend | 01-DM7 | PLANNED | | Run the 402, authorize, fulfill and later settle flow from the web UI; separately run the one-command client script | Full flow works end to end from the web UI | | | The CLI is a supplemental utility and does not count as the frontend |
| ARC-12 | Arc code is open source with a README section and a live demo URL | Arc open source and demo | 01-DM8 | PLANNED | | Publish code, README section and demo URL | Public code and a reachable live demo | | | No URL is entered until a real deployment exists |
| ARC-13 | Arc evidence package: Studio URL, Studio in the demo, frontend and backend, architecture diagram, integration feedback | Arc evidence package | 01-DM9 | PLANNED | | Assemble each listed item in the README and this register | Every listed item present and linked | | | Studio URL is to be rechecked |
| ARC-14 | State machine: lost response, concurrent retries and lease takeover each give one fulfillment and at most one settlement | Arc state machine | 01-DM5 | NOT_YET_RUN | | Run the three scenarios in the spec 01 test list | One stored result; one fulfillment; at most one settlement; stale worker cannot commit | | | A repeated upstream read is allowed under lease takeover |
| ARC-15 | A void authorization is never fulfilled or settled | Arc void authorization | 01-DM5 | NOT_YET_RUN | | Let an accepted authorization expire, then retry it with the same binding | Refused with 409 `authorization_void`; never in a batch; never settled | | |  |
| ARC-16 | Request fingerprint vectors and rejection vectors pass | Arc fingerprint vectors | 01-DM10 | NOT_YET_RUN | | Run vectors 1 to 11 and R1 to R9 from spec 01 against the implementation | Every canonical query, hash and rejection result matches | | | Vectors are computed in the spec, not run here |
| ARC-17 | Settlement batches are claimed atomically, resume with the same ID, and handle rejected and uncertain submissions without a blind duplicate | Arc settlement batches | 01-DM11 | NOT_YET_RUN | | Run two claiming workers, a crash and resume, a Gateway rejection and a lost response | One batch per fulfilled row; same `batch_id` on resume; rejected row `settlement_rejected`; uncertain row reconciled by nonce lookup before any resubmission | | | Per-request onchain settlement is not claimed |
| ARC-18 | Settlement-state mapping and nonce scope are rechecked against current Circle documentation | Arc Circle recheck | 01-DM12 | PLANNED | | Reread the Circle pages cited in spec 01 at implementation and record differences | Differences, if any, recorded and the spec amended | | | Nonce scope is not stated in the pages read on 2026-10-27 |

## World ID IDKit (priority 2, core)

| ID | Claim | Feature | Done means | Status | Commit | Command/procedure | Expected | Actual | Proof | Limitations |
|---|---|---|---|---|---|---|---|---|---|---|
| IDK-01 | A self-service key is minted only after a World ID proof | World ID mint | 02-DM1 | NOT_YET_RUN | | Mint flow with a valid proof | One active self-service free-tier key per action-scoped nullifier | | | Operator-minted keys are a separate documented exception |
| IDK-08 | Cancellation or denial mints no key and writes no nullifier row | World ID cancel or deny | 02-DM2 | NOT_YET_RUN | | Cancel and deny the proof flow | No key; no nullifier row | | |  |
| IDK-02 | A duplicate nullifier is refused | World ID duplicate nullifier | 02-DM3, 02-DM4 | NOT_YET_RUN | | Second mint with the same action-scoped nullifier | Second self-service key refused | | |  |
| IDK-09 | Concurrent mint attempts with one nullifier produce exactly one key | World ID concurrent duplicates | 02-DM4 | NOT_YET_RUN | | Send parallel mint attempts with one action-scoped nullifier | Exactly one key; the others refused | | |  |
| IDK-03 | Lost-key recovery with a fresh proof replaces the key | World ID recovery | 02-DM6 | NOT_YET_RUN | | Recovery with a fresh valid proof | Old key revoked, replacement issued, no new nullifier row | | |  |
| IDK-04 | A revoked key is refused on `/v1` within 5 seconds | World ID revocation `/v1` | 02-DM5, 02-DM7 | NOT_YET_RUN | | Revoke a key and call `/v1` with it | Refused within 5 s | | | Tested upper-bound target under normal operating conditions, not a production SLA |
| IDK-05 | A revoked key's open `/ws` session is closed within 10 seconds | World ID revocation `/ws` | 02-DM5, 02-DM7 | NOT_YET_RUN | | Revoke a key with an open `/ws` session | Session closed within 10 s | | | Tested upper-bound target under normal operating conditions, not a production SLA |
| IDK-10 | Rotation issues a working key and the old key stops working | World ID rotation | 02-DM5 | NOT_YET_RUN | | Rotate a key, then use the old and the new key | New key works; old key refused on `/v1` and `/ws` | | |  |
| IDK-06 | Edge-port bypass is closed | World ID edge-port bypass | 02-DM8 | PLANNED | | External probe from outside the host against `:8081`, `:8090` and `:8091` | Direct access refused, closed or key-checked | | |  |
| IDK-07 | Key checks fail closed | World ID fail-closed | 02-DM9 | PLANNED | | Make the key store unavailable and send requests | Requests refused, not allowed through | | | The README discloses that `cmd/api` fails open today |
| IDK-11 | The human-flow demo mints its key through the self-service IDKit path only | World ID demo path | 02-DM10 | PLANNED | | Record the demo; inspect the key origin | No operator or admin-minted key appears in the demo | | |  |
| IDK-12 | IDKit code is open source with a README section and a live demo | World ID open source and demo | 02-DM11 | PLANNED | | Publish code, README section and demo | Public code and a reachable live demo | | |  |
| IDK-13 | World ID integration debrief is written from real experience | World ID debrief | 02-DM12 | PLANNED | | Write the debrief after the integration is attempted | Time to first success, friction, missing capability or documentation, one high-impact improvement | | | Template only until the integration is attempted |

## World ID Agent Pass (priority 3, targeted)

Separate from the IDKit rows above: IDKit free-tier uniqueness and Agent Pass action approval are separate integrations, and evidence for one is never used for the other. Agent Pass approval is not Arc/x402 payment, so a paid operation also needs its own Arc evidence. Do not enter ID tokens, authorization codes, secrets, request bodies or sensitive query content in any column; use operation IDs and truncated fingerprints only.

| ID | Claim | Feature | Done means | Status | Commit | Command/procedure | Expected | Actual | Proof | Limitations |
|---|---|---|---|---|---|---|---|---|---|---|
| AP-01 | An agent workflow is paused and the exact operation is stored | Agent Pass paused workflow | 04-DM1 | NOT_YET_RUN | | Agent proposes one paid or elevated request; inspect the stored pending operation and try to execute before any approval | Operation stored with account, fingerprint, assurance policy and expiry; protected action not executed | | |  |
| AP-02 | Fresh authentication is requested and validated by the backend | Agent Pass fresh authentication | 04-DM1 | NOT_YET_RUN | | Start the attempt with `max_age=0`; complete World ID authentication; the backend validates state, one-time code redemption, ID token, nonce, issuer, audience, expiry and account/session binding | Result accepted only after all checks; `auth_time` and `acr` meet the saved policy and `amr` equals the saved profile as an unordered set (`["pop"]` baseline; `["pop", "face"]` when Face Auth is required) | | | Mock proofs and fake identities are noted on the prizes page; they do not show real human assurance |
| AP-03 | A valid identity alone does not authorize the action | Agent Pass assurance policy | 04-DM1, 04-DM7 | NOT_YET_RUN | | Authenticate with a wrong `(iss, sub)` for a linked account, with a result below the `acr` or `auth_time` policy, and with the passkey-only profile `amr` `["pop", "user", "mfa"]` (which contains `pop`) and another non-matching `amr` set, under the baseline Orb policy and the Face Auth policy | All rejected; no approval can be recorded; `["pop"]` (baseline) and `["pop", "face"]` (Face Auth) with the correct `acr` are accepted regardless of array order | | |  |
| AP-04 | A separate human approval of the exact action is recorded in our application | Agent Pass action approval | 04-DM1 | NOT_YET_RUN | | After validation, show the approval view (agent/account, method and route, canonical query summary, price or access consequence, fingerprint, expiry) and approve | Approval row bound to the account, operation and displayed fingerprint | | | The World ID issuer authenticates the human; it does not approve the action |
| AP-05 | The approved operation executes exactly once | Agent Pass single execution | 04-DM1, 04-DM6 | NOT_YET_RUN | | Resume the approved operation; recheck, consume the approval and reserve the operation in one transaction | One execution; approval marked used in the same transaction; a paid operation also completes its separate Arc/x402 payment | | | Approval is not payment, fulfillment or settlement; per-request onchain settlement is not claimed |
| AP-06 | A denied operation stays blocked | Agent Pass denial | 04-DM2 | NOT_YET_RUN | | Deny the displayed action, then try to resume | Protected action not executed | | |  |
| AP-07 | Cancelled and expired operations stay blocked | Agent Pass cancel and expiry | 04-DM3 | NOT_YET_RUN | | Cancel one operation and let another expire (including after approval and before execution), then try to resume each | Protected action not executed; expiry not extended by status checks or retries | | |  |
| AP-08 | A backend-validation failure fails closed | Agent Pass validation failure | 04-DM4 | NOT_YET_RUN | | Inject a wrong state, wrong nonce, wrong audience, a callback or approve, deny or cancel request from a different session or account, and an issuer or token endpoint failure, one at a time; also try a human resume from a different session and an agent resume with a different `agent_id` or `account_id` | Attempt failed or request refused; no approval; nothing executed | | |  |
| AP-09 | A changed request needs a new approval | Agent Pass changed fingerprint | 04-DM5 | NOT_YET_RUN | | Change the route, query, price or account after approval and resume | Refused; new proposal and new approval required | | |  |
| AP-10 | Replay and double execution are prevented | Agent Pass replay | 04-DM6 | NOT_YET_RUN | | Replay the callback, code, approval and resume; send two concurrent resumes; start a replacement attempt and then replay the first attempt's callback | At most one execution; others refused; the first attempt is recorded as `superseded` and its callback is refused; no state, nonce or PKCE value is reused across the two attempts | | |  |
| AP-11 | An uncertain outcome is not blindly retried; an idempotent read is resolved by the ledger | Agent Pass stable operation ID | 04-DM6, 04-DM7 | NOT_YET_RUN | | Simulate a lost response after execution starts, for the committed idempotent read and (if one is ever added) for a mutating call | Outcome looked up by operation ID and `auth_key`; no second approval consumed and no repeated authentication; an uncertain mutating outcome is never retried blindly; a repeated idempotent read yields one fulfilled response and at most one settlement | | |  |
| AP-12 | No World ID token is forwarded and no secret reaches logs or evidence | Agent Pass log and token hygiene | 04-DM8 | NOT_YET_RUN | | Review downstream requests, logs and this register for ID tokens, codes, secrets, bodies and sensitive query content | None present | | |  |
| AP-15 | Agent Pass checks are covered by tests, including acceptance and rejection cases | Agent Pass test coverage | 04-DM7 | PLANNED | | Run the test suite for Callback validation, Assurance policy, Execution and Fail-closed outcomes | Both acceptance and rejection cases pass | | |  |
| AP-13 | The approval journey reuses the event web frontend | Agent Pass frontend | 04-DM9 | PLANNED | | Show the approval views inside the event web frontend | No separate unrelated UI | | |  |
| AP-14 | World Agent Pass integration debrief is written from real experience | Agent Pass debrief | 04-DM11 | PLANNED | | Complete the debrief template in the spec after the integration is attempted | Use case, time to first success, friction, missing capability or documentation, one high-impact improvement, what we changed | | | Template only until the integration is attempted |
| AP-16 | Agent Pass code is open source with a README section and a live demo | Agent Pass open source and demo | 04-DM10 | PLANNED | | Publish code, README section and demo | Public code and a reachable live demo | | |  |

## ENS (priority 4, conditional)

| ID | Claim | Feature | Done means | Status | Commit | Command/procedure | Expected | Actual | Proof | Limitations |
|---|---|---|---|---|---|---|---|---|---|---|
| ENS-01 | ENS names are accepted for `/v1/balance` and `/v1/transactions` | ENS name inputs | 03-DM1 | PLANNED | | Attempted only after the Arc, World ID IDKit and World ID Agent Pass flows work end to end with their evidence complete | Names resolve to addresses for the two routes | | | Fourth priority and conditional |
| ENS-02 | Unresolvable and malformed names return the documented errors | ENS errors | 03-DM2 | PLANNED | | Request an unresolvable name and a malformed name | 404 and 400 respectively, with a clear error | | | Not charged on a paid request (spec 01) |
| ENS-03 | Tests cover resolution, cache hits and both error cases | ENS tests | 03-DM3 | PLANNED | | Run the ENS test suite | Resolution, cache-hit and both error cases pass | | |  |
| ENS-04 | Fingerprint tests: a paid ENS request is fingerprinted over the supplied name, and Agent Pass proposals with ENS names are refused | ENS fingerprint | 03-DM4, 01-DM14, 04-DM19 | PLANNED | | Run the ENS fingerprint tests from specs 01, 03 and 04 | Fingerprint over the name; two names resolving to one address differ; a nonce reuse across them gives 409; an Agent Pass proposal with an ENS name is refused with 400 | | |  |
| ENS-05 | ENS code is open source with a README section and a demo | ENS open source and demo | 03-DM5 | PLANNED | | Publish code, README section and demo | Public code and a demo | | | Conditional on ENS work being attempted |
| ENS-06 | No ENSv2 prize claim from ordinary `.eth` resolution | ENS prize eligibility | none (spec section: Partner prize eligibility) | PLANNED | | Review the final integration against the ENS prize criteria | Prize claimed only if ENSv2 on Ethereum mainnet is used in a functional, central way | | | Ordinary `.eth` resolution alone does not support an ENSv2 prize claim |

## Cross-feature (Arc with Agent Pass and ENS)

These rows trace the composition contract in [spec 01](docs/hackathon/01-x402-arc-gateway.md) and [spec 04](docs/hackathon/04-agent-pass.md) and the ENS containment rules. They stay separate from the single-feature rows above. Every row is planned and nothing has been run.

| ID | Claim | Feature | Done means | Status | Commit | Command/procedure | Expected | Actual | Proof | Limitations |
|---|---|---|---|---|---|---|---|---|---|---|
| X-01 | Combined success: a paid Agent-Pass-protected read is approved, reserved, fulfilled once and settled at most once | Arc with Agent Pass: combined success | 04-DM12, 01-DM13 | PLANNED | | Propose, authenticate, approve, hold a payment, resume and fulfill | One approval consumption, one `payment_binding`, one fulfilled response, one fulfillment-eligible authorization, at most one settlement; no World ID token reaches the seller | | | Settlement is evidenced separately by the Arc rows |
| X-02 | Missing payment: no fulfillment without a held authorization, and no bypass by direct payment | Arc with Agent Pass: missing payment | 04-DM13, 01-DM13 | PLANNED | | Resume with no, an unknown or a non-`accepted` `auth_key`; present a payment directly to a protected route | Refused; approval unused; operation stays `approved`; the direct payment is held and never fulfilled | | |  |
| X-03 | Denied approval: a held payment is never fulfilled or settled | Arc with Agent Pass: denied approval | 04-DM14 | PLANNED | | Hold a payment, deny the approval, then try direct payment for the same request | Never fulfilled or settled; reaches `void`; direct attempt refused | | |  |
| X-04 | Reused payment: one authorization serves at most one operation, and an operation has at most one authorization | Arc with Agent Pass: reused payment | 04-DM15 | PLANNED | | Bind an `auth_key` to one operation, then to a second; bind a second key to a bound operation | Both attempts roll back on `UNIQUE`; second approval unused | | |  |
| X-05 | Changed payment: any difference from the operation is refused | Arc with Agent Pass: changed payment | 04-DM16 | PLANNED | | Present authorizations that differ in fingerprint, price, network, asset or `payTo`, one at a time | Each refused with nothing consumed; stored payer equals the ledger payer | | |  |
| X-06 | Cancellation after payment acceptance but before fulfillment, including the cancel and reservation race | Arc with Agent Pass: cancellation | 04-DM17, 04-DM3 | PLANNED | | Cancel with and without naming the held `auth_key`; race a cancellation against a reservation on one `approved` operation; cancel after reservation | No binding; held authorization never fulfilled, batched or settled; exactly one race winner; post-reservation cancel refused | | |  |
| X-07 | Failover during fulfillment of a paid operation, and void after reservation | Arc with Agent Pass: failover | 04-DM18, 01-DM13 | PLANNED | | Fail a worker mid-fulfillment and take over the lease; let a reserved authorization expire before fulfillment | One approval consumption, one fulfilled response, at most one settlement; stale worker cannot commit; void leaves the operation `rejected` with reason `payment_void` | | |  |
| X-08 | The seller and Agent Pass compute one fingerprint | Arc with Agent Pass: shared fingerprint | 01-DM10, 04-DM5 | PLANNED | | Run the same vectors 1 to 11 and R1 to R9 in the seller tests and in the Agent Pass tests | Identical canonical queries and hashes in both | | | Vectors are defined in spec 01 |
| X-09 | Agent Pass accepts literal addresses only; ordinary ENS stays on the two existing routes | Agent Pass with ENS: containment | 04-DM19, 03-DM4 | PLANNED | | Propose an Agent Pass operation with an ENS name and with a literal address; call an ordinary ENS route | ENS name refused with 400 in Agent Pass; literal address accepted; ordinary ENS still resolves on `/v1/balance` and `/v1/transactions` | | |  |

## Traceability to "Done means"

Each spec's "Done means" item is listed with the evidence rows that cover it. A "Done means" item with no row would be a gap; none is listed as uncovered. Listing a row here is not a result: every row is `NOT_YET_RUN` or `PLANNED`.

| Spec | Done means item | Evidence rows |
|---|---|---|
| 01 | 01-DM1 | ARC-01 |
| 01 | 01-DM2 | ARC-02, ARC-03, ARC-06, ARC-07 |
| 01 | 01-DM3 | ARC-10 |
| 01 | 01-DM4 | ARC-09 |
| 01 | 01-DM5 | ARC-04, ARC-05, ARC-14, ARC-15 |
| 01 | 01-DM6 | ARC-08 |
| 01 | 01-DM7 | ARC-11 |
| 01 | 01-DM8 | ARC-12 |
| 01 | 01-DM9 | ARC-13 |
| 01 | 01-DM10 | ARC-16, X-08 |
| 01 | 01-DM11 | ARC-06, ARC-17 |
| 01 | 01-DM12 | ARC-18 |
| 01 | 01-DM13 | X-01, X-02, X-07 |
| 01 | 01-DM14 | ENS-04 |
| 02 | 02-DM1 | IDK-01 |
| 02 | 02-DM2 | IDK-08 |
| 02 | 02-DM3 | IDK-02 |
| 02 | 02-DM4 | IDK-02, IDK-09 |
| 02 | 02-DM5 | IDK-04, IDK-05, IDK-10 |
| 02 | 02-DM6 | IDK-03 |
| 02 | 02-DM7 | IDK-04, IDK-05 |
| 02 | 02-DM8 | IDK-06 |
| 02 | 02-DM9 | IDK-07 |
| 02 | 02-DM10 | IDK-11 |
| 02 | 02-DM11 | IDK-12 |
| 02 | 02-DM12 | IDK-13 |
| 03 | 03-DM1 | ENS-01 |
| 03 | 03-DM2 | ENS-02 |
| 03 | 03-DM3 | ENS-03 |
| 03 | 03-DM4 | ENS-04, X-09 |
| 03 | 03-DM5 | ENS-05 |
| 04 | 04-DM1 | AP-01, AP-02, AP-03, AP-04, AP-05 |
| 04 | 04-DM2 | AP-06 |
| 04 | 04-DM3 | AP-07, X-06 |
| 04 | 04-DM4 | AP-08 |
| 04 | 04-DM5 | AP-09, X-08 |
| 04 | 04-DM6 | AP-05, AP-10, AP-11 |
| 04 | 04-DM7 | AP-03, AP-11, AP-15 |
| 04 | 04-DM8 | AP-12 |
| 04 | 04-DM9 | AP-13 |
| 04 | 04-DM10 | AP-16 |
| 04 | 04-DM11 | AP-14 |
| 04 | 04-DM12 | X-01 |
| 04 | 04-DM13 | X-02 |
| 04 | 04-DM14 | X-03 |
| 04 | 04-DM15 | X-04 |
| 04 | 04-DM16 | X-05 |
| 04 | 04-DM17 | X-06 |
| 04 | 04-DM18 | X-07 |
| 04 | 04-DM19 | X-09, ENS-04 |
