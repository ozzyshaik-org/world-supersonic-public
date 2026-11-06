# x402-seller (experimental seller-engine checkpoint)

Off-chain TypeScript x402 seller for World Supersonic. It is intended to sit in
front of the existing Go API and submit USDC payment authorizations to Circle
Gateway (Arc Testnet) **before** fulfilling upstream.

**This package at `f79de2f` is an experimental seller-engine checkpoint only.**
See [`docs/CHECKPOINT.md`](docs/CHECKPOINT.md) for revision, test counts
(116/116), race-fix evidence boundary, and NOT_RUN items. Unresolved Circle
questions: [`docs/CIRCLE_RECHECK.md`](docs/CIRCLE_RECHECK.md).

Spec: [`docs/hackathon/01-x402-arc-gateway.md`](../docs/hackathon/01-x402-arc-gateway.md).
Event work after `ethglobal-mumbai-2026-start`.

## Checkpoint scope

- Payment state machine and `auth_key` handling
- Canonical x402 v2 payment structure (`payment.accepted` carries selected terms)
- Settle-before-fulfill ordering (no seller batch worker)
- Lease fencing including atomic `resolveRetryableUnderLease`
- Gateway client behind an interface; unit tests use a clearly labelled **FAKE** Gateway
- Fake upstream; **no** live Arc Testnet / Gateway calls in this checkpoint

Out of scope at the engine checkpoint: Arc Studio UI, Agent Pass, ENS, IDKit,
live credentials, root evidence rows, Go API changes, durable store.

A thin local HTTP adapter (experimental, separate commits on
`feat/x402-http-adapter`) may wrap `SellerEngine`; it is **not** live Arc
evidence.

## Commands

```bash
cd x402-seller
npm ci --no-audit --no-fund
npm run typecheck
npm run build
npm test
```

Node engines: `^20.19.0 || >=22.12.0` (aligned with the locked Vite requirement).

## Notes

- `/v1/balance` returns `toString(sum(balance_change))` over
  `worldchain.api_token_balances`. It is **not** native ETH, **not** a wallet
  balance, **not** a per-token wallet balance, and **not** a Gateway balance.
  Keep `/v1/balance` out of paid/ENS demos in this checkpoint.
- The Arc Gateway verifying contract is sourced from Circle's official chain
  configuration (`ARC_TESTNET_GATEWAY_WALLET` in `src/chainConfig.ts`), not an
  unexplained hardcode. See `docs/CIRCLE_RECHECK.md`.
- Only Gateway transfer status `completed` counts as locally settled.
  `received` is not batching evidence; direct batching evidence requires
  `batched` or `confirmed`.
- The Circle adapter is an injected-interface wrapper only. It is **not** a
  verified live integration.
- Evidence boundary for the race fix at `f79de2f`: the pre-fix proof is
  **ledger-level**; a pre-fix full engine-level 2xx rejection was **not**
  independently reproduced. No live Arc/Circle behaviour verified.
