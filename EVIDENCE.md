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
| ARC-02 | A signed authorization is validated, reserved and accepted by Gateway through `settle()` | Arc x402 acceptance | 01-DM2 | NOT_YET_RUN | | Retry with a signed payment authorization; inspect the ledger row and the Gateway response | Request validated; ledger row created in `submission_pending`; `settle()` returns success; row `accepted` with a Gateway transfer ID | | | Acceptance means Gateway verified the authorization, locked the funds and queued it; it is not fulfillment, batching or settlement |
| ARC-03 | An accepted request is fulfilled after acceptance, and the response is not delayed for settlement | Arc x402 fulfillment | 01-DM2 | NOT_YET_RUN | | Same request after acceptance; record the time of `settle()` success, the upstream call and the response | Real answer from the Go API with block context, response and receipt stored and returned at once; the upstream call is after `settle()` success and does not wait for Gateway batching | | | Fulfillment is not settlement |
| ARC-04 | Same-request retry is not charged twice | Arc x402 idempotent retry | 01-DM5 | NOT_YET_RUN | | Resend the same request with the same nonce and binding in the accepted, fulfilled, batched and completed states | Stored response and receipt returned; no upstream call, no Gateway `settle()` call, no second fulfillment | | |  |
| ARC-05 | Nonce reuse with a different binding is rejected | Arc x402 nonce conflict | 01-DM5 | NOT_YET_RUN | | Reuse a nonce with a different fingerprint, payer, price, asset, network, `payTo`, `validAfter` or `validBefore`, one case per field | HTTP 409 `authorization_binding_mismatch` for each field, including `validAfter` and `validBefore`; no upstream call, no Gateway call, no state change, no charge | | |  |
| ARC-06 | Gateway batches the accepted transfer, observed by reconciliation by nonce | Arc x402 Gateway batching | 01-DM8 | NOT_YET_RUN | | Look up the transfer by nonce (network passed explicitly) after acceptance and record each status seen with its time | Transfer found with status `received`, `batched` or `confirmed`; the ledger records the status; the seller creates, claims and submits no batch | | | Batching is Gateway's own and is not settlement. Status history depends on when lookups are made |
| ARC-07 | The transfer reaches `completed`, which is final settlement | Arc x402 Gateway settlement | 01-DM8 | NOT_YET_RUN | | Gateway transfer lookup and status evidence for the transfer; ledger state after a reconciliation pass | Transfer `completed`; the ledger marks it settled only at `completed`; at most one settlement per authorization | | | Per-request onchain settlement is not claimed. Not replaceable by a recipient-wallet balance check |
| ARC-08 | Paid routes cannot be bypassed | Arc paid-route enforcement | 01-DM9 | PLANNED | | External probe from outside the host against `:8081` and other proxy paths | Direct access to a paid route is refused or closed, or the route is a documented free tier | | |  |
| ARC-09 | An invalid authorization is not fulfilled and does not reach the Go API or Gateway | Arc x402 invalid authorization | 01-DM4 | NOT_YET_RUN | | Send a malformed, wrongly signed and expired authorization | Each refused before any ledger row or `settle()` call where it fails local validation; no upstream call; no fulfillment | | |  |
| ARC-10 | The Arc Studio project is the starting point of the seller service | Arc Studio origin | 01-DM3 | PLANNED | | Show the Studio project and the seller service derived from it | Project exists and the seller service starts from it | | | The real generated project URL (on studio.arc.io) is recorded here once the project exists |
| ARC-11 | The Studio-built web UI performs the full flow against the seller service | Arc Studio frontend | 01-DM10 | PLANNED | | Run the 402, authorize, accepted, fulfilled flow from the web UI, then show Gateway batching and completion as later events; separately run the one-command client script | Full flow works end to end from the web UI | | | The CLI is a supplemental utility and does not count as the frontend |
| ARC-12 | Arc code is open source with a README section and a live demo URL | Arc open source and demo | 01-DM11 | PLANNED | | Publish code, README section and demo URL | Public code and a reachable live demo | | | No URL is entered until a real deployment exists |
| ARC-13 | Arc evidence package: Studio URL, Studio in the demo, frontend and backend, architecture diagram, integration feedback | Arc evidence package | 01-DM12 | PLANNED | | Assemble each listed item in the README and this register | Every listed item present and linked | | | The real generated project URL is recorded when the project exists |
| ARC-14 | State machine: lost response, concurrent retries and lease takeover each give one fulfillment and one acceptance | Arc state machine | 01-DM5 | NOT_YET_RUN | | Run the three scenarios in the spec 01 test list | One stored result; one fulfillment; one `settle()` acceptance; stale worker cannot commit | | | A repeated upstream read is allowed under lease takeover |
| ARC-15 | No void after Gateway acceptance; `void` only for never-accepted authorizations | Arc void rules | 01-DM7 | NOT_YET_RUN | | Let `validBefore` pass and fail the upstream read after acceptance; separately let a never-submitted authorization pass its window and retry it | The accepted authorization is not voided and is still fulfilled by a later read; the never-accepted one is `void` and a same-binding retry is refused with 409 `authorization_void`; the void row is never fulfilled | | |  |
| ARC-16 | Request fingerprint vectors and rejection vectors pass | Arc fingerprint vectors | 01-DM13 | NOT_YET_RUN | | Run vectors 1 to 11 and R1 to R9 from spec 01 against the implementation | Every canonical query, hash and rejection result matches | | | Vectors are computed in the spec, not run here |
| ARC-17 | Uncertain submission is reconciled by nonce before any resubmission or fulfillment | Arc submission uncertainty | 01-DM6 | NOT_YET_RUN | | Make `settle()` time out, drop and return an empty body, and race concurrent requests; look up by nonce before any retry | State `submission_uncertain`; no fulfillment and no resubmission before the lookup; when the transfer exists there is no second submission; one submitter among concurrent requests; `nonce_already_used` reconciled and not treated as a rejection | | | Per-request onchain settlement is not claimed |
| ARC-18 | Gateway status mapping, `transaction` and transfer ID, nonce scope and validity-window rule are rechecked against current Circle documentation | Arc Circle recheck | 01-DM14 | PLANNED | | Reread the Circle pages cited in spec 01 at implementation and record differences | Differences, if any, recorded and the spec amended | | | Nonce scope and the validity-window measurement point are not stated in the pages read on 2026-10-27 |
| ARC-19 | `settle()` is called before the upstream fulfillment (settle-before-serve) | Arc settle-before-serve ordering | 01-DM2 | NOT_YET_RUN | | Run an open-route request with a call log on the seller, `settle()` and the Go API; also fail `settle()` and confirm the Go API is not called | Log order is validate, reserve, `settle()` success, upstream call, response; with `settle()` failing, no upstream call | | | Order of calls only; not an onchain result |
| ARC-20 | A definite rejection before acceptance means no fulfillment and no resubmission | Arc definite rejection | 01-DM4 | NOT_YET_RUN | | Cause a definite `settle()` rejection (for example an insufficient Gateway balance) and retry with the same nonce | No upstream call; ledger `rejected`; 402 `payment_rejected`; the retry does not resubmit; a new nonce is required | | | Depends on being able to provoke a rejection with real test funds |
| ARC-21 | Upstream retry after acceptance gives one stored fulfillment; a permanent upstream failure is reported honestly | Arc post-acceptance upstream | 01-DM7 | NOT_YET_RUN | | Fail the upstream read transiently after acceptance and retry; separately return a non-retryable upstream error | Transient: one stored fulfillment and no second `settle()`; permanent: `paid_unfulfilled`, 502, and no claim of void or uncharged | | | The remedy for a permanent failure is operator policy and is not specified |
| ARC-22 | A failed Gateway transfer is handled by when it is found: before fulfillment no fulfillment, after fulfillment an incident | Arc failed transfer | 01-DM8 | NOT_YET_RUN | | Reconcile a transfer reported `failed` before fulfillment and one reported `failed` after fulfillment | Before: not fulfilled, 402 `payment_failed`; after: stored response preserved, `fulfilled_but_unpaid` incident raised, no resubmission | | | Depends on being able to observe a failed transfer |

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
| AP-04 | A separate human approval of the exact action is recorded in our application | Agent Pass action approval | 04-DM1, 04-DM22 | NOT_YET_RUN | | After validation, show the approval view (agent/account, method and route, canonical query summary, payment amount, network, asset and `payTo` or the access consequence, fingerprint, expiry) and approve | Approval row bound to the account, operation and displayed fingerprint; the view is rendered from the stored snapshot | | | The World ID issuer authenticates the human; it does not approve the action |
| AP-05 | The approved operation executes exactly once | Agent Pass single execution | 04-DM1, 04-DM6 | NOT_YET_RUN | | Resume the approved operation (for a paid one, the paid protected request); recheck, then consume the approval, reserve the operation and create the binding and ledger row in one transaction | One execution; approval marked used in the same transaction; a paid operation commits its reservation before `settle()` and then completes its separate Arc/x402 payment | | | Approval is not payment, fulfillment or settlement; per-request onchain settlement is not claimed |
| AP-06 | A denied operation stays blocked | Agent Pass denial | 04-DM2 | NOT_YET_RUN | | Deny the displayed action, then try to resume | Protected action not executed | | |  |
| AP-07 | Cancelled and expired operations stay blocked | Agent Pass cancel and expiry | 04-DM3 | NOT_YET_RUN | | Cancel one operation and let another expire (including after approval and before execution), then try to resume each | Protected action not executed; expiry not extended by status checks or retries | | |  |
| AP-08 | A backend-validation failure fails closed | Agent Pass validation failure | 04-DM4 | NOT_YET_RUN | | Inject a wrong state, wrong nonce, wrong audience, a callback or approve, deny or cancel request from a different session or account, and an issuer or token endpoint failure, one at a time; also try a human resume from a different session and an agent resume with a different `agent_id` or `account_id` | Attempt failed or request refused; no approval; nothing executed | | |  |
| AP-09 | A changed request or payment terms need a new approval | Agent Pass changed fingerprint | 04-DM5, 04-DM16 | NOT_YET_RUN | | Change the route, query, payment network, asset, `payTo`, amount or account after approval and resume | Refused before any `settle()` call; new proposal and new approval required | | |  |
| AP-10 | Replay and double execution are prevented | Agent Pass replay | 04-DM6 | NOT_YET_RUN | | Replay the callback, code, approval and resume; send two concurrent resumes; start a replacement attempt and then replay the first attempt's callback | At most one execution; others refused; the first attempt is recorded as `superseded` and its callback is refused; no state, nonce or PKCE value is reused across the two attempts | | |  |
| AP-11 | An uncertain outcome is not blindly retried; an idempotent read is resolved by the ledger | Agent Pass stable operation ID | 04-DM6, 04-DM7 | NOT_YET_RUN | | Simulate a lost response after execution starts, for the committed idempotent read and (if one is ever added) for a mutating call | Outcome looked up by operation ID and `auth_key`; no second approval consumed, no second binding and no repeated authentication; an uncertain mutating outcome is never retried blindly; a repeated idempotent read yields one fulfilled response | | |  |
| AP-12 | No World ID token is forwarded and no secret reaches logs or evidence | Agent Pass log and token hygiene | 04-DM8 | NOT_YET_RUN | | Review downstream requests, logs and this register for ID tokens, codes, secrets, bodies and sensitive query content | None present | | |  |
| AP-15 | Agent Pass checks are covered by tests, including acceptance and rejection cases | Agent Pass test coverage | 04-DM7 | PLANNED | | Run the test suite for Callback validation, Assurance policy, Execution and Fail-closed outcomes | Both acceptance and rejection cases pass | | |  |
| AP-13 | The approval journey reuses the event web frontend | Agent Pass frontend | 04-DM9 | PLANNED | | Show the approval views inside the event web frontend | No separate unrelated UI | | |  |
| AP-14 | World Agent Pass integration debrief is written from real experience | Agent Pass debrief | 04-DM11 | PLANNED | | Complete the debrief template in the spec after the integration is attempted | Use case, time to first success, friction, missing capability or documentation, one high-impact improvement, what we changed | | | Template only until the integration is attempted |
| AP-16 | Agent Pass code is open source with a README section and a live demo | Agent Pass open source and demo | 04-DM10 | PLANNED | | Publish code, README section and demo | Public code and a reachable live demo | | |  |
| AP-17 | Application session requirements: allowlisted origins, cookie attributes, expiry, rotation, and no session identifiers in URLs or logs | Agent Pass session | 04-DM20 | PLANNED | | Send a credentialed request from a disallowed origin; inspect the session cookie; use an expired and a rotated-out session for start, approve, deny, resume and cancel; review URLs, logs and evidence | Disallowed origin refused; cookie has `Secure`, `HttpOnly` and `SameSite`; expired and rotated-out sessions refused for every action; no session identifier in URLs, logs or evidence | | |  |

## ENS (priority 4, conditional)

| ID | Claim | Feature | Done means | Status | Commit | Command/procedure | Expected | Actual | Proof | Limitations |
|---|---|---|---|---|---|---|---|---|---|---|
| ENS-01 | ENS names are accepted for `/v1/balance` and `/v1/transactions` | ENS name inputs | 03-DM1 | PLANNED | | Attempted only after the Arc, World ID IDKit and World ID Agent Pass flows work end to end with their evidence complete | Names resolve to addresses for the two routes | | | Fourth priority and conditional |
| ENS-02 | Unresolvable and malformed names return the documented errors without a charge | ENS errors | 03-DM2 | PLANNED | | Request an unresolvable name and a malformed name on a free route and on a paid route | 404 and 400 respectively, with a clear error; on a paid route the failure happens at the preflight before `settle()`, so there is no ledger row, no `settle()` call and no charge | | | Spec 01 defines the paid-route preflight |
| ENS-03 | Tests cover resolution, cache hits and both error cases | ENS tests | 03-DM3 | PLANNED | | Run the ENS test suite | Resolution, cache-hit and both error cases pass | | |  |
| ENS-04 | Fingerprint and preflight tests: a paid ENS request is fingerprinted over the supplied name, resolved before `settle()`, fulfilled with the recorded address, and Agent Pass proposals with ENS names are refused | ENS fingerprint and preflight | 03-DM4, 01-DM16, 04-DM19 | PLANNED | | Run the ENS fingerprint and preflight tests from specs 01, 03 and 04 | Fingerprint over the name; resolution before `settle()`; the upstream call carries the recorded address and a later resolution change does not alter it; two names resolving to one address differ; a nonce reuse across them gives 409; an Agent Pass proposal with an ENS name is refused with 400 | | |  |
| ENS-05 | ENS code is open source with a README section and a demo | ENS open source and demo | 03-DM5 | PLANNED | | Publish code, README section and demo | Public code and a demo | | | Conditional on ENS work being attempted |
| ENS-06 | No ENSv2 prize claim from ordinary `.eth` resolution | ENS prize eligibility | none (spec section: Partner prize eligibility) | PLANNED | | Review the final integration against the ENS prize criteria | Prize claimed only if ENSv2 on Ethereum mainnet is used in a functional, central way | | | Ordinary `.eth` resolution alone does not support an ENSv2 prize claim |

## Cross-feature (Arc with Agent Pass and ENS)

These rows trace the composition contract in [spec 01](docs/hackathon/01-x402-arc-gateway.md) and [spec 04](docs/hackathon/04-agent-pass.md) and the ENS containment rules. They stay separate from the single-feature rows above. Every row is planned and nothing has been run. The seller submits payment to Gateway before it fulfills, so the operation is reserved before `settle()`.

| ID | Claim | Feature | Done means | Status | Commit | Command/procedure | Expected | Actual | Proof | Limitations |
|---|---|---|---|---|---|---|---|---|---|---|
| X-01 | Combined success: a paid Agent-Pass-protected read is approved, reserved, then submitted, fulfilled once and accepted once | Arc with Agent Pass: combined success | 04-DM12, 01-DM15 | PLANNED | | Propose, authenticate, approve, sign the exact payment, send the paid protected request; record the reservation commit, `settle()` and upstream times | Reservation commits before `settle()`; one approval consumption, one `payment_binding`, one ledger row, one fulfilled response, one acceptance; no World ID token reaches the seller | | | Gateway batching and final settlement are evidenced separately by ARC-06 and ARC-07 |
| X-02 | Payment without a valid reservation is refused before `settle()` | Arc with Agent Pass: no reservation | 04-DM13, 01-DM15 | PLANNED | | Present a paid request with no operation identifier, an unknown operation, a non-`approved` operation and a stale snapshot; present a payment directly to a protected route | Refused with 403 `agent_pass_operation_required` or 409 `agent_pass_operation_stale`; no ledger row; no `settle()` call; approval unused; nothing accepted or fulfilled | | |  |
| X-03 | Denied or expired approval: nothing is submitted and no payment waits idle | Arc with Agent Pass: denied or expired approval | 04-DM14 | PLANNED | | Sign a payment, deny (or let expire) the approval, then present the paid request and a direct payment for the same request | No ledger row and no `settle()` call at any point; both later attempts refused before `settle()` | | | The client's signed authorization is simply never submitted |
| X-04 | Reused payment: one authorization serves at most one operation, and an operation has at most one authorization | Arc with Agent Pass: reused payment | 04-DM15 | PLANNED | | Bind an `auth_key` to one operation, then present it for a second; present a second authorization for a bound operation | Both attempts roll back or are refused before any `settle()` call; second approval unused | | |  |
| X-05 | Changed payment: any difference from the stored snapshot is refused | Arc with Agent Pass: changed payment | 04-DM16 | PLANNED | | Present authorizations that differ in fingerprint, network, asset, `payTo` or amount, one at a time; change the price table after proposal | Each refused (409 `agent_pass_operation_stale`) with nothing consumed and no `settle()` call; the snapshot fields are never updated; the binding copies them | | |  |
| X-06 | Cancellation before the reservation, refusal after it (including after acceptance), and the cancel and reservation race | Arc with Agent Pass: cancellation | 04-DM17, 04-DM3 | PLANNED | | Cancel before any paid request; race a cancellation against a reservation on one `approved` operation; cancel after the reservation and again after Gateway acceptance | Before: no binding, no ledger row, nothing submitted; exactly one race winner; after the reservation and after acceptance: refused with state unchanged | | | This spec offers no way to stop an accepted authorization from being batched |
| X-07 | Failover during the upstream read of a paid operation after acceptance | Arc with Agent Pass: failover | 04-DM18, 01-DM15 | PLANNED | | Fail a worker mid-fulfillment and take over the lease; let `validBefore` pass after acceptance | One approval consumption, one logical fulfilled response, one acceptance; stale worker cannot commit; no void after acceptance | | |  |
| X-08 | The seller and Agent Pass compute one fingerprint | Arc with Agent Pass: shared fingerprint | 01-DM10, 04-DM5 | PLANNED | | Run the same vectors 1 to 11 and R1 to R9 in the seller tests and in the Agent Pass tests | Identical canonical queries and hashes in both | | | Vectors are defined in spec 01 |
| X-09 | Agent Pass accepts literal addresses only; ordinary ENS stays on the two existing routes | Agent Pass with ENS: containment | 04-DM19, 03-DM4 | PLANNED | | Propose an Agent Pass operation with an ENS name and with a literal address; call an ordinary ENS route | ENS name refused with 400 in Agent Pass; literal address accepted; ordinary ENS still resolves on `/v1/balance` and `/v1/transactions` | | |  |
| X-10 | One-to-one binding holds under concurrent resumes | Arc with Agent Pass: concurrent resumes | 04-DM18 | PLANNED | | Send concurrent paid requests and resumes for one `approved` operation, and concurrent presentations of one authorization | Exactly one reservation, one approval consumption, one `payment_binding`, one `settle()` submitter; the others refused or served the stored result | | |  |
| X-11 | Definite payment rejection fails closed; uncertain submission recovers on the same binding | Arc with Agent Pass: payment outcomes | 04-DM21, 01-DM6 | PLANNED | | Cause a definite `settle()` rejection after the reservation; separately make `settle()` time out and recover with the same authorization | Rejection: operation `rejected` (`payment_rejected`), approval stays consumed, new proposal and approval required. Uncertain: operation stays `executing`, reconciled by nonce, one binding and no second approval | | |  |
| X-12 | Post-acceptance failures are reported honestly: a permanent upstream failure, and a transfer that fails after fulfillment | Arc with Agent Pass: post-acceptance failure | 04-DM21, 01-DM7, 01-DM8 | PLANNED | | Return a non-retryable upstream error after acceptance; reconcile a transfer reported `failed` after fulfillment | Operation `failed` with `paid_unfulfilled` and no claim of void; for the late transfer failure the operation stays `executed`, the stored response is preserved and `fulfilled_but_unpaid` is raised with no resubmission | | |  |
| X-13 | The approved payment snapshot is shown with an integer-derived amount and is immutable | Agent Pass: approved snapshot | 04-DM22, 04-DM16 | PLANNED | | Render the approval view for operations with several amounts; attempt to update the snapshot fields | Amount derived by integer arithmetic from base units (no floats); fields written once at proposal and never updated | | |  |

## Traceability to "Done means"

Each spec's "Done means" item is listed with the evidence rows that cover it. A "Done means" item with no row would be a gap; none is listed as uncovered. Listing a row here is not a result: every row is `NOT_YET_RUN` or `PLANNED`.

| Spec | Done means item | Evidence rows |
|---|---|---|
| 01 | 01-DM1 | ARC-01 |
| 01 | 01-DM2 | ARC-02, ARC-03, ARC-19 |
| 01 | 01-DM3 | ARC-10 |
| 01 | 01-DM4 | ARC-09, ARC-20 |
| 01 | 01-DM5 | ARC-04, ARC-05, ARC-14 |
| 01 | 01-DM6 | ARC-17, X-11 |
| 01 | 01-DM7 | ARC-15, ARC-21, X-12 |
| 01 | 01-DM8 | ARC-06, ARC-07, ARC-22, X-12 |
| 01 | 01-DM9 | ARC-08 |
| 01 | 01-DM10 | ARC-11 |
| 01 | 01-DM11 | ARC-12 |
| 01 | 01-DM12 | ARC-13 |
| 01 | 01-DM13 | ARC-16, X-08 |
| 01 | 01-DM14 | ARC-18 |
| 01 | 01-DM15 | X-01, X-02, X-07 |
| 01 | 01-DM16 | ENS-04 |
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
| 04 | 04-DM16 | X-05, X-13, AP-09 |
| 04 | 04-DM17 | X-06 |
| 04 | 04-DM18 | X-07, X-10 |
| 04 | 04-DM19 | X-09, ENS-04 |
| 04 | 04-DM20 | AP-17 |
| 04 | 04-DM21 | X-11, X-12 |
| 04 | 04-DM22 | AP-04, X-13 |
