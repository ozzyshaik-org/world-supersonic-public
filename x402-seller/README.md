# x402-seller (first checkpoint)

Off-chain TypeScript x402 seller for World Supersonic. It sits in front of the
existing Go API and submits USDC payment authorizations to Circle Gateway
(Arc Testnet) **before** fulfilling upstream.

This directory is event work after `ethglobal-mumbai-2026-start`. Spec:
[`docs/hackathon/01-x402-arc-gateway.md`](../docs/hackathon/01-x402-arc-gateway.md).

## First checkpoint scope

- Payment state machine and `auth_key` handling
- Canonical x402 v2 payment structure (`payment.accepted` carries selected terms)
- Settle-before-fulfill ordering (no seller batch worker)
- Gateway client behind an interface; unit tests use a clearly labelled **FAKE** Gateway
- Fake upstream; no live Arc Testnet / Gateway calls in this checkpoint

Out of scope here: Arc Studio UI, Agent Pass, ENS, IDKit, live credentials,
evidence rows, Go API changes, HTTP server surface, durable store.

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
