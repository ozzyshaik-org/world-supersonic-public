# Feature 1: x402 pay-per-request access (Arc, Circle Gateway, USDC)

Status: committed scope; specification only

Official kickoff on 5 Nov 2026; implementation is event work after the start tag.

## Goal
Let any client, human or AI agent, pay per request for World Chain data in USDC, with no account or API key.

## Design
- An offchain TypeScript x402 seller service, created with Arc Studio (studio.arc.io), sits in front of the existing Go API. The service connects to Arc Testnet for USDC payments through Circle Gateway. It is not an onchain program and is not "deployed on" Arc; where it is hosted is chosen at implementation.
- The Go API and `cmd/rpc-gateway` are unchanged. The seller service calls the Go API over HTTP with one server-side API key held in its environment, and calls no other application-data upstream. Circle Gateway remains the payment service: the seller calls its `settle()` to submit a payment authorization and its transfer lookup to observe the result. Gateway carries no application data.
- **Circle's documented sequence (re-read 2026-10-27; quotes in the table below)** is: the seller submits the signed authorization; Gateway verifies it, locks the buyer's funds and queues it for batch processing; the seller serves the resource immediately, without waiting for onchain settlement; Gateway, not the seller, later performs the batch and the onchain settlement. This spec follows that order: **the seller submits through `settle()` before upstream fulfillment**, never after it.
- **Seller flow for an ordinary open paid route:**
  1. A client requests a paid `/v1` route with no payment. The seller responds `402 Payment Required` with machine-readable payment requirements: price, asset (USDC), network (Arc Testnet) and `payTo` address.
  2. The client signs a USDC payment authorization (Circle Gateway nanopayments) and retries the request with the payment header.
  3. **Validate before settle.** The seller validates the request shape (method, body, path, query and the fingerprint rules below, and for an ENS name the resolution preflight) and the payment's structure (required fields, a 32-byte nonce, and network, asset, `payTo` and amount equal to the price table). Any failure is refused with a `4xx` or `503` before any ledger row exists and before any Gateway call, so it is never charged.
  4. **Reserve.** The seller inserts the ledger row for the `auth_key` in one atomic write with payment state `submission_pending` and fulfillment state `not_started`.
  5. **Submit.** The seller calls Gateway `settle()` with the signed authorization. A successful `settle()` means Gateway accepted the authorization: it verified it, locked the buyer's funds and queued it for batch processing. It does **not** mean the batch has been submitted or has settled onchain.
  6. **Fulfill.** Only after acceptance is known, the seller calls the Go API under a lease, stores the response and a receipt, and returns the response at once. It does not wait for Gateway's batch or for onchain settlement.
  7. **Later, by Gateway.** Gateway batches the accepted transfer with others and settles onchain on its own schedule. The seller does not create, claim, group or submit any batch. It observes the outcome only by transfer reconciliation (lookup by nonce; see Gateway batching and reconciliation).
- A definite `settle()` rejection is never fulfilled. A submission with an unknown outcome is reconciled by nonce before anything else happens (see the state model).
- A per-payer rate limit stops one funded wallet from flooding the Go API.
- Per-route price table: path to price in USDC atomic base units (an integer string; USDC has 6 decimals, so one cent is `10000`). No floating-point amount is used anywhere in the specs or the ledger. The human-readable form is derived by integer arithmetic for display only.
- The committed paid read flow is `GET` with an empty body. Other methods and bodies are outside it (see Request fingerprint).
- Per-request onchain settlement is not claimed. **Four events are separate and each needs its own evidence:** (1) Gateway accepts the authorization (`settle()` succeeds), (2) the seller fulfills the request upstream, (3) Gateway batches the transfer (observed through transfer status), and (4) the transfer is `completed` (final settlement). A successful response to the client proves only events 1 and 2.
- The demo has a public URL.
- The frontend is a web UI built with Arc Studio, and the backend is the seller service. A client uses the web UI to request a paid route, sign the payment authorization and see the fulfilled response and receipt. The one-command CLI demo client is a supplemental test utility, not the sole frontend.

## Authorization key
The ledger needs one stable identifier per payment authorization. The identifier is chosen from what Circle's documentation says about the nonce (all pages read on 2026-10-27):

| Source | What it says |
|---|---|
| [Sign EIP-3009 payment authorizations](https://developers.circle.com/gateway-nanopayments/howtos/eip-3009-signing) | The `nonce` is "a unique random 32-byte value for every payment"; reusing one is rejected. The EIP-712 domain is `GatewayWalletBatched` with the chain ID and the Gateway Wallet address (`verifyingContract`). `validBefore` must be at least 3 days ahead, or the authorization is rejected as `authorization_validity_too_short`. |
| [Gateway Nanopayments SDK reference](https://developers.circle.com/sdks/gateway-nanopayments-sdk) | `BatchFacilitatorClient.settle(payload, requirements)`: "Submits a payment for batched settlement through the Gateway API." Its `SettleResponse` is `{ success, errorReason?, payer?, transaction }`. Error `nonce_already_used`: "Payment nonce was already submitted". `TransferStatus` is `"received" \| "batched" \| "confirmed" \| "completed" \| "failed"`. `searchTransfers` can filter by `nonce` and by default searches the client's own network; `getTransferById` takes the transfer UUID. `unexpected_error`: "Infrastructure error (HTTP 500)". `Circle Gateway settle returned empty response ({status})` is a documented client error. |
| [Reconcile nanopayments as a seller](https://developers.circle.com/gateway-nanopayments/howtos/x402-seller-reconciliation) | "a completed request on your server confirms only that the payment was accepted, not that it settled". "nonce is unique per payment, so this returns at most one transfer". "Wait for `confirmed` before crediting the payer for accounting purposes and `completed` before initiating any dependent onchain operation. Transfers that end in `failed` were never charged. Do not fulfill the resource in that case." |
| [How batched settlement works](https://developers.circle.com/gateway-nanopayments/concepts/batched-settlement) | Step 4: "The seller (or a facilitator) submits the signed authorization to Gateway. Gateway verifies the signature, locks the buyer's funds, and credits the seller's pending balance. The seller serves the resource immediately, without waiting for onchain settlement." Step 5: "Gateway periodically collects pending authorizations, computes net balance changes across all participants, and submits a single onchain transaction that applies the changes. After onchain confirmation, pending balances become available." |
| [Settle an x402 payment](https://developers.circle.com/api-reference/gateway/all/settle-x402payment) | "Settles an x402 payment by submitting the EIP-3009 authorization. The authorization will be verified, the sender's balance locked, and the transaction queued for batch processing." Outcome is the `success` field of an HTTP 200 body; `transaction` is a UUID on success and an empty string on failure; HTTP 400 is an invalid or malformed request and HTTP 500 is `unexpected_error`. `errorReason` is one of `unsupported_scheme`, `unsupported_network`, `unsupported_asset`, `invalid_payload`, `address_mismatch`, `amount_mismatch`, `invalid_signature`, `authorization_not_yet_valid`, `authorization_expired`, `authorization_validity_too_short`, `self_transfer`, `insufficient_balance`, `nonce_already_used`, `unsupported_domain` or `wallet_not_found`. |
| [EIP-3009](https://eips.ethereum.org/EIPS/eip-3009) | In the base standard, a used nonce is tracked per authorizer: `authorizationState(authorizer, nonce)`. |

**What is not stated:** Circle's pages say the nonce is unique per payment but do not say whether uniqueness is per payer, per chain or global inside Gateway. The base EIP-3009 standard scopes it to the payer on the token contract. This is **to be confirmed against official docs at implementation**.

**Decision (conservative):**
- The stable **authorization key** is the composite `auth_key = <network>|<payer>|<nonce>`: the network exactly as in the accepted payment requirements, the payer address in lowercase `0x` hex, and the nonce as lowercase `0x` plus 64 hex characters, joined by `|`. It is the ledger's primary key and the identifier other components use, so it makes no assumption about Gateway's scope.
- The ledger also keeps a `UNIQUE` index on `nonce` alone. A nonce that is already present under a different payer or network is therefore refused (see the binding rules below) before it reaches `settle()`, instead of being treated as a new authorization that Gateway would reject as `nonce_already_used`.
- If Circle confirms per-payer or per-chain scope, relaxing the `nonce` index is a spec amendment, not a silent change.
- A nonce that is not exactly 32 bytes is refused as invalid.

## Request fingerprint
There is exactly one fingerprint algorithm, version `v1`. The seller, the ledger and the Agent Pass operation record in [04](04-agent-pass.md) all compute it with the same function, and the same vectors below are tests in each.

**Scope.** The committed paid read flow is `GET` with an empty body, which fits every route the initial scope uses (`/v1/balance`, `/v1/transactions` and the other read routes in `docs/openapi.yaml` are `GET`). A paid route that takes a request body (for example `POST /v1/auth/keys`) is outside the committed flow. Adding one needs a spec amendment that defines body hashing and a new fingerprint version.

**Rules.**
1. Inputs are the method, the raw request path and the raw query string. Headers (including the payment header and the seller's server-side API key), cookies and the URL fragment are not inputs.
2. **Method:** only `GET`. Any other method on a paid route is refused with `405`, before any fingerprint is computed.
3. **Body:** the request must carry zero body bytes. Any body, even on `GET`, is refused with `400`.
4. **Path:** the raw path must equal a route string in the price table byte for byte, otherwise `404` (no `402` challenge). Nothing is decoded or normalized, so a path with `%`, `//`, a dot segment, a trailing `/` or a different letter case matches no route.
5. **Query parsing:** the raw query is the text after the first `?`; a missing query and an empty query are the same (empty). It must be ASCII with no space, control character or `#`, else `400`. Split on `&`. An empty segment (`a=1&&b=2`, a leading or trailing `&`) is `400`. Split each segment at its first `=`; a segment with no `=`, or with an empty name, is `400`.
6. **Decoding:** percent-decode each name and each value exactly once. `%` must be followed by two hex digits in either case, else `400`. `+` is a literal plus sign, not a space. The decoded bytes must be valid UTF-8, else `400`. No Unicode normalization and no case folding is applied, including to hex addresses.
7. **Names:** every name must be a documented parameter of that route in `docs/openapi.yaml`, else `400`. A name that appears twice after decoding is `400`; duplicates are never merged or reordered.
8. **Encoding:** re-encode each decoded name and value from its UTF-8 bytes. The characters `A-Z a-z 0-9 - . _ ~` stay as they are; every other byte becomes `%XX` with uppercase hex digits. An empty value stays empty (`limit=` is kept, not dropped).
9. **Order:** sort the pairs by the encoded name, comparing bytes, ascending. Join them as `name=value` with `&`, with no leading `?`. No pairs gives an empty string.
10. **Canonical string:** five lines joined by a single line feed (`0x0A`) with no trailing line feed: the label `supersonic-x402-fp-v1`; the method; the path; the canonical query (an empty line when there is none); the literal `body:empty`. A line feed never occurs inside a component (rule 8 encodes one in a value as `%0A`; a method or path cannot contain one), so no component can be shifted across a boundary.
11. **Fingerprint:** SHA-256 of the UTF-8 bytes of the canonical string, written as 64 lowercase hex characters.
12. Any change to these rules gets a new version label. Fingerprints of different versions are never compared.

**Test vectors.** The hashes were computed with a standard SHA-256 tool over the exact canonical string, for example:

```
printf 'supersonic-x402-fp-v1\nGET\n/v1/balance\naddress=0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045\nbody:empty' | sha256sum
```

| # | Raw request target | Canonical query (line 4) | SHA-256 |
|---|---|---|---|
| 1 | `/v1/balance?address=0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045` | `address=0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045` | `ba886fc0227c7de9fec80680d996b742ce885d36acd97586a5356cd5a8beec59` |
| 2 | `/v1/transactions?limit=10&address=0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045` (also `address=...&limit=10`, and `limit=%31%30`) | `address=0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045&limit=10` | `d3314b3faa2a2a11d83b5f514d1d1ec618962591f3dcc770eb2e1e22139ff950` |
| 3 | `/v1/transactions?address=0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045&limit=` (empty value kept) | `address=0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045&limit=` | `7a16783af6912cf6c09a03ae11ad4ad7171ef16eecd440a27f41273d006f10ca` |
| 4 | `/v1/block/latest` (also `/v1/block/latest?`) | empty line | `e8e05191f29720d69a7662f65ca8c0b9076e8dadfeb9eec249ef884c94f9f317` |
| 5 | `/v1/defi/events?type=a%20b` | `type=a%20b` | `04d6ea2c43828ae69a7f7bf04d7430a9a53e785a3f9fb1eadabf2fd0ec18c183` |
| 6 | `/v1/defi/events?type=a+b` (also `type=a%2bb`) | `type=a%2Bb` | `e3ab72ae336a01e60dafcc38094db160bc37f27a93720962fb429bd7d7651acf` |
| 7 | `/v1/defi/events?type=a%0ab` (a line feed stays encoded) | `type=a%0Ab` | `1a373336c7664e4e6f9a3e5940ce276b61420547766db6c9f973ceb1fa9beace` |
| 8 | `/v1/defi/events?type=%c3%a9` | `type=%C3%A9` | `4a5d45e031dd0ccc18eeaa6fd7972502cdda7d4432aa88598369cd7e452a8def` |
| 9 | `/v1/balance?address=0xd8da6bf26964af9d7eed9e03e53415d37aa96045` (lowercase address: a different request from vector 1) | `address=0xd8da6bf26964af9d7eed9e03e53415d37aa96045` | `6acd3e7a7953ad7ded8fa951d93e494a3e4c1976a6d8750f095bb218da39bc31` |

Vectors 5 and 6 differ on purpose: a space and a plus sign are different values. Vectors 1 and 9 differ on purpose: the fingerprint does no address case folding.

**Rejection vectors** (no fingerprint is produced, no `402` challenge, no upstream call):

| # | Request | Result |
|---|---|---|
| R1 | `GET /v1/transactions?limit=1&limit=2` | `400` duplicate name |
| R2 | `GET /v1/transactions?address=0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045&&limit=1` | `400` empty segment |
| R3 | `GET /v1/transactions?limit` | `400` segment without `=` |
| R4 | `GET /v1/defi/events?type=%zz` | `400` bad percent escape |
| R5 | `GET /v1/defi/events?type=%FF` | `400` not valid UTF-8 |
| R6 | `GET /v1/balance/?address=0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045` | `404` no route matches the path |
| R7 | `GET /v1/balance?address=0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045&x=1` | `400` undocumented parameter |
| R8 | `GET /v1/balance?address=0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045` with a one-byte body | `400` body not empty |
| R9 | `POST /v1/balance` | `405` method outside the committed paid flow |

## ENS names and the fingerprint
Ordinary ENS support ([03](03-ens-integration.md)) stays on `/v1/balance` and `/v1/transactions`. **Decision:** for a paid request on those routes, the fingerprint covers the ENS name exactly as the client supplied it, in its canonical-query form. It does not cover the address the name resolves to.

- The fingerprint is computed before any resolution and is the same on every retry.
- What the payer authorized is therefore fixed by the request they signed for, and cannot be changed by a later change to the name's mutable ENS records.
- Names are compared as received. `Vitalik.eth` and `vitalik.eth` are different requests with different fingerprints, and no ENS normalization is applied inside the fingerprint (vectors 10 and 11). Any normalization of the name happens inside the resolution preflight below and does not alter the stored fingerprint.
- The same payment authorization for a different name that happens to resolve to the same address is a different request: the nonce is refused with `409` (`authorization_binding_mismatch`).
- Switching to a resolved-address fingerprint later is a spec amendment with a new fingerprint version.

**Resolution preflight (before `settle()`).** Because the seller now submits payment before it fulfills, a name that cannot resolve has to be caught before submission, or the client would be charged for an error. Resolution therefore runs as a preflight, ahead of the ledger reservation and ahead of `settle()`:
- *When.* After request-shape and payment-structure validation, and only for a request that carries a structurally valid payment. An unpaid request gets the `402` challenge with no resolution, so unpaid traffic cannot cause resolver calls. Preflights are rate limited per client address and per payer.
- *How.* The seller asks the Go API to resolve the name with the same resolver, mainnet RPC endpoint and cache as spec 03. The preflight returns only the resolved address, its resolution time and the block; it returns no application data and is not fulfillment. The Go API's preflight surface (a resolve-only route or an equivalent) is documented in [03](03-ens-integration.md) and chosen at implementation.
- *Failure is free.* A `404` (name does not resolve) or `400` (malformed name) is returned to the client unchanged. A transient resolver failure or timeout is `503`. In all three cases there is no ledger row, no `settle()` call and no charge, and the client's signed authorization is unspent.
- *Record the result.* On success, the resolved address, the resolution time and the block are written in the same atomic insert that reserves the ledger row, once, and never changed. These three values are recorded with the binding but are **not** part of the binding comparison: a retry of the same authorization with the same name never resolves again and is judged on the fingerprint alone.
- *Fulfillment uses the recorded address.* The upstream call to the Go API carries the recorded literal address in place of the name, so every attempt (the first fulfillment, a retry, a lease takeover and a failover) queries the same address and no second resolution happens anywhere. The response to the client states the supplied name, the recorded address and the resolution time and block, as in spec 03.
- *If the resolution changes later.* If the name's records change between the preflight and fulfillment, or later, nothing changes for this authorization: the recorded preflight address wins, and the stored response is for that address. The response says when the name was resolved, so the client can see the answer is as of that time. A new request with a new nonce preflights afresh.
- *Concurrent first arrivals.* Two arrivals may each run a preflight and may differ if the records changed between them. The atomic insert decides: the winner's recorded address is the only one used, and the loser's result is discarded.
- *What the preflight does not cover.* A failure of the Go API on the literal-address fulfillment call after Gateway acceptance (for example an upstream `5xx`) is an ordinary post-acceptance upstream failure, with no relation to ENS, handled by the state model.

- Agent Pass operations do not use ENS names: they accept literal addresses only (see [04](04-agent-pass.md)).

| # | Raw request target | Canonical query (line 4) | SHA-256 |
|---|---|---|---|
| 10 | `/v1/balance?address=vitalik.eth` | `address=vitalik.eth` | `36407a0aefb250af55ad8cd129236a8b87d9a19481733d76951522fa1f2f4d0d` |
| 11 | `/v1/balance?address=Vitalik.eth` | `address=Vitalik.eth` | `07d110ae01fb259a82cba9acdad88f147257aba96f0e154370476ebc1580e39e` |

## Ledger and authorization binding
The ledger has one row per authorization key. The insert of a new key is atomic (it is also the reservation), so concurrent first arrivals produce one row. Each row holds the **authorization binding**, written once at insert and never changed:

| Field | Meaning |
|---|---|
| `auth_key`, `nonce` | The composite key above, and the nonce alone (also unique) |
| `network`, `asset`, `pay_to`, `price` | As signed and as required by the price table: network, USDC token address, seller `payTo` address, amount in base units |
| `payer` | The `from` address of the authorization |
| `valid_after`, `valid_before` | The signed validity window |
| `fingerprint` | The `v1` fingerprint of the request the authorization was accepted for |
| `resolved_address`, `resolved_at`, `resolved_block` | ENS routes only: the preflight result, written once; recorded with the binding but not compared (see Resolution preflight) |

The row also holds the mutable state fields (payment state, Gateway status and fulfillment state, defined below), the submission attempt counter and timestamps, the lease owner and expiry, the stored response and receipt once fulfilled, the Gateway fields (`gateway_transfer_id`, taken from the `transaction` value that `settle()` returns, `gateway_status`, `gateway_error`) and the stored payment payload that submission needs. The payload is never logged or returned to a client. There is no seller `batch_id`: the seller does not form batches.

**Binding rule.** A request whose nonce is already in the ledger is a retry of the same authorization only if its payer, network, asset, `payTo`, price, `validAfter`, `validBefore` and fingerprint all equal the stored binding. If any one differs, including `validAfter` alone or `validBefore` alone, the request is refused with `409` and the code `authorization_binding_mismatch`, in every state. There is no Gateway call, no upstream call, no state change and no charge. A request that matches a stored binding is served from the ledger. It is never submitted to `settle()` again, because Circle rejects a resubmitted nonce, except in the one case the state model allows: a `submission_uncertain` row that reconciliation proved was never accepted.

## Payment state and fulfillment state
A recorded nonce is not a consumed nonce, and a fulfilled request is not a settled payment. The ledger therefore keeps three independent fields instead of one linear chain, because payment and fulfillment move on different clocks and can fail separately.

- **`payment_state`**: where the submission to Gateway stands. Values: `submission_pending`, `submission_uncertain`, `accepted`, `rejected`, `void`.
- **`gateway_status`**: what Gateway reports about an accepted transfer, copied only from a lookup (see Gateway batching and reconciliation). Values: none, `received`, `batched`, `confirmed`, `completed`, `failed`.
- **`fulfillment_state`**: where the upstream read stands. Values: `not_started`, `in_progress`, `fulfilled`, `retryable_failure`, `failed_permanent`.

```mermaid
stateDiagram-v2
    direction LR
    [*] --> submission_pending: reserved (valid shape, new key)
    submission_pending --> accepted: settle() success, transaction stored
    submission_pending --> rejected: definite rejection
    submission_pending --> submission_uncertain: timeout, drop, empty body, unexpected error, crash
    submission_pending --> void: validity closed, nothing submitted
    submission_uncertain --> accepted: lookup finds the matching transfer
    submission_uncertain --> rejected: lookup finds a different transfer on the nonce
    submission_uncertain --> submission_pending: lookup finds nothing, validity open (resubmit)
    submission_uncertain --> void: nothing found, validity closed, grace elapsed
```

```mermaid
stateDiagram-v2
    direction LR
    [*] --> not_started
    not_started --> in_progress: payment accepted, worker takes lease
    in_progress --> fulfilled: upstream 2xx, response and receipt stored
    in_progress --> retryable_failure: upstream error, timeout or lease expiry
    retryable_failure --> in_progress: same-request retry takes lease
    in_progress --> failed_permanent: non-retryable upstream answer or retry budget spent
    retryable_failure --> failed_permanent: retry budget spent
```

The two diagrams are joined only by guards: the fulfillment diagram may leave `not_started` only while `payment_state = accepted` and `gateway_status` is not `failed`. Nothing in the payment diagram depends on fulfillment, and `gateway_status` can change at any time after acceptance.

**Rules.**
1. **No fulfillment before acceptance.** Nothing calls the Go API for a row until `payment_state = accepted`.
2. **Validate before settle.** Malformed input, a non-matching price, network, asset or `payTo`, an unknown route, and for ENS routes an unresolvable name, are all refused before the reservation. They never reach `settle()`.
3. **Definite rejection before acceptance.** `settle()` answered HTTP 200 with `success: false` and an `errorReason`, or HTTP 400 (invalid or malformed request). The row moves to `rejected` with the code stored. There is no upstream call, and nothing is retried automatically. The client gets `402` with the code `payment_rejected` and Gateway's `errorReason`, and signs a new authorization with a new nonce. `rejected` is terminal; nothing was accepted.
4. **Uncertain submission.** A timeout, a dropped connection, an empty response body, an HTTP 500 or `unexpected_error`, `nonce_already_used` on any submission, or a crash between "attempt recorded" and "outcome recorded" is **not** a rejection and **not** an acceptance. The row is `submission_uncertain`. Reconcile by nonce (below) before anything is resubmitted or fulfilled. While unresolved, the client gets `503` with the code `payment_submission_unresolved` and `Retry-After`.
5. **Only one submitter.** The submitter is the request that moves the attempt counter from `n` to `n+1` with a conditional write, which also stores the attempt time. Only that request calls `settle()`. Another request for the same key, even after the first one's process died, never calls `settle()` while an attempt is unresolved: it follows rule 4. A `submission_pending` row with an attempt counter of zero (reserved but never submitted) can be claimed by any same-binding request the same way.
6. **After acceptance, upstream reads may retry.** Reads are idempotent and repeat under the existing lease fencing. `validBefore` is irrelevant once Gateway has accepted the authorization: Gateway holds it. An upstream read failure is a fulfillment event, never a payment event.
7. **`void` applies only before acceptance.** `void` means no authorization reached Gateway's acceptance: the validity window closed while the row was `submission_pending`, or reconciliation proved an uncertain submission was never accepted. A row that is `accepted` is never moved to `void`, whether `validBefore` passes, an upstream read fails or a client cancels.
8. **Reconciliation by nonce.** Look up the transfer as described in Gateway batching and reconciliation, with `from`, `to`, amount and nonce compared to the binding.
   - A transfer that matches: adopt its identifier and status; `payment_state` becomes `accepted` and `gateway_status` is copied. There is no second submission.
   - A transfer on the nonce whose sender, recipient or amount differ: the nonce was consumed by a different payment. The row becomes `rejected` (`nonce_consumed_elsewhere`). Nothing is fulfilled.
   - The lookup succeeds and finds nothing, and the validity window is open: the row may go back to `submission_pending` and be resubmitted. Each resubmission is a new attempt, preceded by a fresh lookup that found nothing, and bounded by an attempt cap chosen at implementation. Gateway's nonce check (`nonce_already_used`) is a backstop and never the plan; if it answers, the row goes back to `submission_uncertain` and reconciliation repeats.
   - The lookup succeeds, finds nothing, the validity window has closed and a grace period has elapsed: after at least two such lookups, spaced apart, the row becomes `void`. The search's consistency is not documented, so the grace period and the lookup count are conservative and **to be confirmed against official docs at implementation**.
   - The lookup fails, is ambiguous, or the bounds above are not met: the row stays `submission_uncertain` and is flagged `submission_unresolved` for an operator. No blind duplicate submission, no fulfillment.
9. **Gateway status mapping.** `received`, `batched` and `confirmed` mean accepted and not yet settled. `completed` means locally settled and is the only settled state. `failed` means the payment failed.
10. **A failed transfer before fulfillment means no fulfillment.** If `gateway_status = failed` is found while `fulfillment_state` is `not_started` or `retryable_failure`, the seller does not call the Go API. The client gets `402` with the code `payment_failed`.
11. **A failed transfer after fulfillment is an incident, not a rewrite.** If `gateway_status = failed` is found when `fulfillment_state = fulfilled`, the stored response and receipt are preserved unchanged, the row is flagged `fulfilled_but_unpaid`, and an operator is told. The seller does not erase the history, does not resubmit the authorization, and never records the request as not fulfilled.
12. **Permanent upstream failure after acceptance.** If the Go API gives a non-retryable answer to a request that already passed validation, or the retry budget (a count and a total time chosen at implementation) is spent, `fulfillment_state` becomes `failed_permanent` and the row is flagged `paid_unfulfilled`. This spec does **not** say the payment was void or uncharged: Gateway accepted the authorization and locked the buyer's funds, and the transfer continues through Gateway's own batching unless Gateway reports `failed`. The client gets `502` with the code `paid_unfulfilled` and the `auth_key`, and no further automatic upstream calls are made. What to do for the payer (a credit or a refund outside Gateway, or an operator re-opening the row to `retryable_failure`) is an operator decision that this spec does not automate. The pages read do not describe any seller-initiated cancel or refund of an accepted Gateway transfer; that is **to be confirmed against official docs at implementation**. The seller may reduce the exposure with a cheap upstream readiness check before `settle()`, which is not a guarantee.

**Charge wording.** Acceptance, fulfillment, batching and settlement are different things, and only the last is final.
- *Accepted* means Gateway verified the authorization, locked the buyer's funds and queued it. It is not yet a settled charge, and it is not reversible by this spec.
- *Fulfilled* means exactly one logical response was stored and delivered.
- *Charged* (locally settled) means `gateway_status = completed`. Until then nothing is claimed as settled.
- Wherever this spec or the evidence log says "one charge", it means one accepted authorization, one logical fulfilled response, and at most one settlement.

| `payment_state` | Meaning | Charge status |
|---|---|---|
| `submission_pending` | Reserved; `settle()` has not answered. A recorded attempt with no outcome is treated as uncertain | Not submitted |
| `submission_uncertain` | The outcome of a submission is unknown and is being reconciled | Unknown; not fulfilled |
| `accepted` | Gateway accepted the authorization: verified, funds locked, queued | Accepted; settled only when `gateway_status = completed`; never charged if `failed` |
| `rejected` | Gateway definitively refused it, or the nonce was consumed by another payment | Not charged; terminal |
| `void` | Nothing was ever accepted and the authorization can no longer be accepted | Not charged; terminal |

| From | To | Trigger |
|---|---|---|
| none | `submission_pending` | Valid shape and payment structure, key and nonce not in the ledger; the atomic insert is the reservation |
| `submission_pending` | `accepted` | `settle()` returns `success: true` with a `transaction` value |
| `submission_pending` | `rejected` | HTTP 200 with `success: false`, or HTTP 400 |
| `submission_pending` | `submission_uncertain` | Rule 4 |
| `submission_pending` | `void` | The validity window closes before any attempt was recorded |
| `submission_uncertain` | `accepted`, `rejected`, `submission_pending`, `void` | Rule 8 |

| `fulfillment_state` | Meaning |
|---|---|
| `not_started` | No upstream call has been made for this key |
| `in_progress` | One worker holds a lease and is calling the Go API |
| `fulfilled` | Upstream response and receipt stored |
| `retryable_failure` | The upstream call failed, timed out or lost its lease; the response is not stored |
| `failed_permanent` | Rule 12; flagged `paid_unfulfilled` |

| From | To | Trigger |
|---|---|---|
| `not_started` | `in_progress` | A worker takes the lease with a compare-and-set that also requires `payment_state = accepted` and `gateway_status` not `failed` (and, for an Agent-Pass-protected route, the composition contract) |
| `in_progress` | `fulfilled` | Go API returns success; response and receipt stored in one transaction under the lease (`validBefore` is not a condition) |
| `in_progress` | `retryable_failure` | Go API error or timeout, or the lease expires without a stored response |
| `retryable_failure` | `in_progress` | A same-binding request takes the lease, after a status lookup that did not report `failed` |
| `in_progress`, `retryable_failure` | `failed_permanent` | Rule 12 |

Incident flags are derived and stored beside the state: `fulfilled_but_unpaid` (rule 11), `paid_unfulfilled` (rule 12) and `submission_unresolved` (rule 8). An incident never changes a stored response.

**Same nonce, same binding.** The binding rule above applies first. For a request with the same binding:

| Row | Result |
|---|---|
| `submission_pending`, attempt counter zero | The request becomes the submitter (rule 5) and continues as a first arrival |
| `submission_pending` with an unresolved attempt, or `submission_uncertain` | No `settle()` call. Reconcile (rule 8); until resolved, `503` `payment_submission_unresolved` |
| `accepted`, `not_started` or `retryable_failure`, `gateway_status` not `failed` | Take the lease, call the Go API and fulfill. One fulfillment in total |
| `accepted`, `in_progress` | Wait for the lease holder and return its stored response; if the lease expires first, take it over as for `retryable_failure`. Duplicate upstream reads may happen after a takeover but never produce a second fulfillment or settlement |
| `accepted`, `fulfilled`, any `gateway_status` | Return the stored response and receipt. No upstream call, no Gateway submission, no second charge. A `failed` status keeps its incident flag; the response is not changed |
| `accepted`, not fulfilled, `gateway_status = failed` | No upstream call; `402` `payment_failed` |
| `accepted`, `failed_permanent` | The stored `502` `paid_unfulfilled`; no upstream call |
| `rejected` | The stored `402` `payment_rejected`. No `settle()` call; the client needs a new authorization and nonce |
| `void` | Terminal. `409` `authorization_void`. The client signs a new authorization with a new nonce |
| a different binding in any state | `409` `authorization_binding_mismatch`. No upstream call, no Gateway call, no state change, no charge |

**Lease fencing:** fulfillment is committed with a conditional write that checks both the key and the current lease owner. A worker that no longer holds a valid lease must not store or directly return its upstream result as the fulfilled response. It discards that result and either returns the current stored result or instructs the client to retry. Every successful caller for the same key and binding receives the same stored response and receipt.

## Gateway batching and reconciliation
Gateway, not the seller, collects accepted authorizations, batches them and submits the onchain transaction. The seller never creates, claims, groups or submits a batch, and the ledger holds no batch identifier of its own. The seller only observes: it looks up the transfer for each accepted authorization and records what Gateway reports.

- **Lookup.** By transfer UUID (`getTransferById`) when the `transaction` value from `settle()` is stored, otherwise by `nonce` through the transfer search, with the sending network passed explicitly (the search defaults to the client's own network) and with `from`, `to`, amount and nonce compared to the stored binding. A transfer is adopted only if its sender, recipient, amount and nonce equal the stored binding. How `amount` is represented in a transfer record (decimal or base units) and whether `transaction` equals the transfer `id` are **to be confirmed against official docs at implementation**.
- **Gateway status mapping.** `gateway_status` only moves forward and is never inferred, only copied from a lookup:

| Gateway status | Meaning for this ledger | Locally settled? |
|---|---|---|
| (none yet) | `settle()` succeeded and no lookup has run | No |
| `received` | Gateway has the accepted transfer | No |
| `batched` | Gateway has put it in a batch | No |
| `confirmed` | The batch is confirmed onchain per Gateway | No (see below) |
| `completed` | Final settlement | **Yes, the only settled state** |
| `failed` | The transfer failed; Circle says such transfers "were never charged" | No; a payment failure (see below) |

  Only `completed` counts as locally settled. This is stricter than Circle's seller how-to, which says to wait for `confirmed` before crediting the payer for accounting purposes and for `completed` before a dependent onchain operation. Nothing in this project credits an account or runs a dependent onchain operation, so the stricter reading costs nothing, and promoting `confirmed` is a spec amendment. A status that moves backwards or is not in the list is recorded as an anomaly and never changes the stored state.
- **When lookups run.** Right after acceptance is not required (acceptance is the `settle()` success). A lookup runs: before each retry of a failed fulfillment; during reconciliation of an uncertain submission; and periodically for every accepted row that is not `completed` or `failed`. The period is chosen at implementation.
- **Failed transfer.** If the status is `failed` and the row is not yet fulfilled (`not_started` or `retryable_failure`), the seller does **not** fulfill: the row is closed as a payment failure, not retried and the client gets `402` with the code `payment_failed`. If the row is already fulfilled, the stored response is preserved and the row is flagged `fulfilled_but_unpaid` (see the state model). It is never blindly resubmitted and never recorded as not fulfilled.
- **Evidence.** Acceptance, fulfillment, `batched`/`confirmed` and `completed` are four separate evidence events (see Design).

## Composition with Agent Pass (spec 04)
This section is the seller's half of the contract; [04](04-agent-pass.md) holds the application's half (the `payment_binding` record, the execution transaction and cancellation). Both halves describe the same rules.

- **Route classes.** Every price-table entry is either `open` (payment is enough, as above) or `agent_pass` (payment is not enough). A route is never both. A paid operation that Agent Pass protects targets only an `agent_pass` route, so presenting payment through an `open` entry cannot stand in for the checkpoint. Which routes are `agent_pass` is chosen at implementation; the initial Agent Pass flow uses literal-address `GET` requests only.
- **Payment alone never fulfills a protected route.** The seller verifies and records a payment for an `agent_pass` route as usual (state `accepted`), but it does not take the lease. It answers `403` with the code `agent_pass_reservation_required` and the `auth_key`. Nothing is fulfilled, nothing is fulfillment-eligible, and the authorization can never enter a settlement batch.
- **Reservation check.** The `accepted` or `retryable_failure` to `in_progress` transition for such a row is one conditional write that also requires: a `payment_binding` row for this `auth_key` in state `reserved`; the bound `pending_operation` in state `executing`; and the operation's fingerprint, price, network, asset and `payTo`, and the binding's payer, equal to the ledger binding. The seller reads only application state (operation ID, state, fingerprint, price). **No World ID token, authorization code or claim is sent to or read by the seller**, and none is forwarded downstream.
- **One transaction boundary.** The conditional write above, the execution transaction in 04 and the cancellation in 04 must be able to commit atomically against one another, so the ledger and the Agent Pass tables share one transactional store. If an implementation cannot meet this, the composition is not implemented as specified and this spec is amended first.
- **Fulfillment commit.** When the ledger moves to `fulfilled`, the `payment_binding` moves to `fulfilled` and the operation to `executed` in the same transaction.
- **Void after reservation.** If the validity window closes after the reservation but before fulfillment, the ledger row moves to `void`, the binding to `void`, and the operation to `rejected` (reason `payment_void`). It is never fulfilled or settled. A new proposal, a new approval and a new authorization are required.
- **Cancellation before the reservation commits.** The operation is `cancelled`, and no binding can ever be created for it. If the cancellation names a held `auth_key`, the same transaction moves that row from `accepted` to `void`, conditional on no binding existing. If it does not, the unbound authorization still cannot be fulfilled and reaches `void` when its validity closes. Cancellation after the reservation has committed is refused (04), and the ledger then follows the normal lease and fulfillment rules.
- **Retries.** Idempotent upstream reads may repeat under lease fencing, as above. Across the whole composition there is at most one approval consumption, one binding, one logical fulfilled response, one fulfillment-eligible authorization and one settlement. A mutating upstream call is outside the committed flow; if one is ever added, an uncertain outcome of it is never retried blindly.
- **Settlement is separate.** `executed` means fulfilled, not settled. Settlement follows the batch rules above and is never claimed from the Agent Pass side.

## Paid-route enforcement
A route represented as paid is reachable only through the seller. Direct access to the Go API on `:8081` and any other proxy path to the same route is closed to the public network, or the route is a deliberately different free tier that the README and this spec document. The evidence is an external probe from outside the host.

## Done means
1. A request to a paid `/v1` route without payment returns HTTP 402 with machine-readable payment requirements.
2. The client retries with a signed payment authorization; the seller verifies it and fulfills the request; fulfilled authorizations are later settled in batch through Circle Gateway on Arc.
3. The Arc Studio project, created with Arc Studio, is the starting point of the seller service.
4. A request with an invalid authorization is not fulfilled and does not reach the Go API.
5. State machine tests cover:
   - Retry after a lost response: the upstream call completes, the response is lost before it is stored, and the retry with the same nonce and fingerprint is fulfilled with one fulfillment and at most one settlement.
   - Concurrent same-request retries, when no lease failure occurs: parallel requests with one nonce and fingerprint produce one upstream call, one fulfillment, at most one settlement and the same response for every caller.
   - Lease takeover failover: the first worker loses its lease while its upstream read is still running and a second worker takes over. A repeated upstream read is allowed. The test requires one stored result, one fulfillment and at most one settlement, and fails on a second fulfillment or settlement. The stale worker's late completion cannot overwrite the stored result or produce a second successful response. Concurrent and failover callers receive the same stored response and receipt, with one fulfillment and at most one settlement.
   - Nonce reuse with a different binding: the same nonce with a different fingerprint, payer, price, asset, network or `payTo` is rejected with `409` (`authorization_binding_mismatch`), one case per field, and none reaches the Go API or Circle Gateway.
   - Retry of a fulfilled, batched or settled authorization with the same binding returns the stored response and receipt with no upstream call, no Gateway call and no second charge.
   - A voided authorization is never fulfilled or settled, and a same-binding retry of a void authorization is refused with `409` (`authorization_void`).
6. Bypass evidence: an external probe from outside the host, with its output recorded in the evidence, shows direct access to `:8081` and any other proxy path to a paid route is refused or closed; or the route is a documented free tier.
7. The Arc Studio-built web UI performs the full flow end to end against the seller service. A demo client script also performs the full flow in one command as a supplemental test utility.
8. Open-source code, README section and a live demo URL.
9. Arc evidence is recorded in the README and the evidence log:
   - The Arc Studio URL (studio.arc.io) of the project.
   - Arc Studio shown in the demo.
   - An Arc Studio-built web frontend (web UI) and a backend (the seller service), both shown working together in the demo. The one-command CLI is a supplemental test utility and does not count as the frontend.
   - An architecture diagram of client, seller service, Circle Gateway, Go API and Arc.
   - Integration feedback on Arc Studio, Circle Gateway and Arc Testnet.
10. Fingerprint tests: every test vector (1 to 11) produces the listed canonical query and SHA-256, and every rejection vector (R1 to R9) produces the listed result and no `402` challenge.
11. Settlement batch tests:
    - Two settlement workers claim at the same time: each fulfilled authorization joins exactly one batch, with one `batch_id`, and no row is claimed twice.
    - A batch resumed after a crash keeps its `batch_id` and its members.
    - A member that Gateway rejects ends `settlement_rejected`, is never counted as settled or regrouped, and does not change the other members.
    - A submission whose response is lost ends `submission_uncertain` and is reconciled by a lookup on nonce; when the transfer exists there is no second submission. A resubmission happens only after a successful lookup that finds nothing.
    - `nonce_already_used` on a resubmission is treated as uncertain, not as a rejection.
12. The settlement-state mapping (`received`, `batched`, `confirmed`, `completed`, `failed` to ledger states) and the nonce scope are re-checked against Circle's current documentation at implementation, and any difference is recorded.
13. Agent Pass composition tests (spec 04 lists the full set): payment presented directly for an `agent_pass` route is never fulfilled; an authorization held without a reservation is never batched; a lease takeover on an `agent_pass` route passes the reservation check for both workers and still gives one fulfillment and at most one settlement; a void after reservation is never fulfilled or settled.
14. ENS fingerprint tests (PLANNED, ordinary routes only): the fingerprint of a request with an ENS name equals vectors 10 and 11 and is computed over the name, not the resolved address; two names that resolve to the same address have different fingerprints, and reusing one authorization's nonce for the other name is refused with `409`; a change in what the name resolves to between attempts changes neither the fingerprint nor an already stored response; an unresolvable name returns the upstream error without a fulfilled result or a charge.
