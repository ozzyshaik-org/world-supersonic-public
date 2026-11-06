# x402-seller experimental checkpoint (`f79de2f`)

**Status:** EXPERIMENTAL seller-engine checkpoint only. Not a live Arc / Circle
integration. Not a public HTTP surface (HTTP adapter is a separate follow-up).
Not submission-ready evidence for live Gateway, Arc Testnet, batching,
settlement, Arc Studio, or end-to-end paid demos.

**Revision:** `f79de2f36b8a07371b072398ea1e2657775c3a2f`
(`fix(x402-seller): atomic retryable/budget CAS under original lease`, head of
draft PR #23 / `feat/arc-x402-seller` at the time of this note).

## What exists at this revision

- Canonical x402 v2 payment structure and local validation
  (`payment.accepted` holds selected terms; 32-byte nonce; safe-integer validity;
  Circle 3-day `validBefore` floor).
- Settle classification (definite reject vs uncertain / malformed →
  `submission_uncertain`).
- Fail-closed transfer reconciliation (central matching; empty/ambiguous/failed
  lookups do not auto-accept).
- Lease fencing, including atomic `resolveRetryableUnderLease` (the race fix in
  this revision).
- In-memory ledger, FAKE Gateway, FAKE upstream. No durable store. No live
  Circle / Arc Testnet calls. No credentials used.

Unresolved Circle/docs questions remain in
[`CIRCLE_RECHECK.md`](./CIRCLE_RECHECK.md).

## Local unit / contract evidence (owner-verified)

Labels:

- **unit-fake** — Gateway is the package `FakeGatewayClient` (synthetic).
- **contract-real-local** — `circleImport` exercises the real installed
  `@circle-fin/x402-batching` / `@x402/evm` import surface with **no network**.

Commands (from `x402-seller/`):

```bash
npm ci --no-audit --no-fund
npm run typecheck
npm run build
npm test
```

Result recorded by the owner and by CI:

| Check | Result |
| --- | --- |
| typecheck | passed (owner) |
| build | passed (owner) |
| `npm test` | **116/116** passed |
| CI run | [42180463805](https://github.com/ozzyshaik-org/world-supersonic-public/actions/runs/42180463805) success |

Test counts by file (116 total):

| File | Count | Label |
| --- | --- | --- |
| `fingerprint.test.ts` | 21 | unit-fake |
| `authKey.test.ts` | 3 | unit-fake |
| `paymentStructure.test.ts` | 27 | unit-fake |
| `stateMachine.test.ts` | 20 | unit-fake |
| `regressionMatrix.test.ts` | 38 | unit-fake |
| `circleImport.test.ts` | 2 | contract-real-local (no network) |
| `leaseRaceBudget.test.ts` | 5 | unit-fake |

## Race-fix evidence (`f79de2f`)

- New ledger regression (stale unfenced `markFailedPermanent` after
  `markRetryableFailure` release) **fails on** prefix `dc761fd` with the
  expected `toBeNull` assertion: received a non-null row with
  `fulfillmentState: "failed_permanent"`, incident `paid_unfulfilled`,
  `leaseGeneration: 2`. Captured output:
  `/workspace/pr23-race-prefix.txt`.
- Same test **passes on** `f79de2f`.
- All five race tests in `leaseRaceBudget.test.ts` pass on `f79de2f`.
- Owner independently verified typecheck, build, 116/116, the five race tests,
  and the pre-fix failure on `dc761fd`.

### Evidence boundary (explicit)

The pre-fix proof is **ledger-level**. A pre-fix full **engine-level** 2xx
rejection was **not** independently reproduced. Do not claim otherwise.

No live Arc / Circle behaviour has been verified at this checkpoint.

## Not claimed / NOT_RUN

- Live Circle Gateway `settle()` / transfer lookup
- Arc Testnet payments, batching (`batched`/`confirmed`), or `completed`
  settlement
- Arc Studio project / UI
- Real HTTP adapter / public deploy / durable store (follow-up work)
- IDKit, Agent Pass, ENS, pstack formal verification

## Follow-up: experimental local HTTP adapter (`feat/x402-http-adapter`)

Thin `node:http` adapter around `SellerEngine` (not live Arc evidence):

- Real HTTP 402 with `PAYMENT-REQUIRED` (base64) + JSON `{ x402Version, accepts }`
- `PAYMENT-SIGNATURE` parse; malformed → 400 before Gateway/upstream
- `Retry-After` on unresolved / retryable 503
- `HttpUpstreamClient`: `redirect: "manual"`, AbortController timeout, server-side
  `X-API-Key` only
- Allowlisted paid route only (`/v1/mev/analytics` by default); no generic proxy
- Same-binding stored-response replay over HTTP

### Local HTTP evidence labels

| Suite | Label |
| --- | --- |
| `test/httpAdapter.local-http-integration.test.ts` | **local-http-integration + unit-fake Gateway** |
| `test/httpHeaders.test.ts` | unit-fake |
| `test/httpPaymentHeader.property.test.ts` | unit-fake (fast-check) |

**NOT_RUN / MISSING:** live Gateway, Arc Testnet, batching, settlement `completed`,
Arc Studio, public deploy, durable store, IDKit / Agent Pass / ENS.
