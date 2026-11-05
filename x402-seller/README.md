# x402-seller (first checkpoint)

Off-chain TypeScript x402 seller for World Supersonic. It sits in front of the
existing Go API and submits USDC payment authorizations to Circle Gateway
(Arc Testnet) **before** fulfilling upstream.

This directory is event work after `ethglobal-mumbai-2026-start`. Spec:
[`docs/hackathon/01-x402-arc-gateway.md`](../docs/hackathon/01-x402-arc-gateway.md).

## First checkpoint scope

- Payment state machine and `auth_key` handling
- Settle-before-fulfill ordering (no seller batch worker)
- Gateway client behind an interface; unit tests use a clearly labelled **FAKE** Gateway
- Fake upstream; no live Arc Testnet / Gateway calls in this checkpoint

Out of scope here: Arc Studio UI, Agent Pass, ENS, IDKit, live credentials,
evidence rows, Go API changes.

## Commands

```bash
cd x402-seller
npm install
npm run typecheck
npm test
```

## Notes

- `/v1/balance` is an application data route (address balance on World Chain).
  It is **not** a wallet or Gateway balance endpoint.
- Only Gateway transfer status `completed` counts as locally settled.
  `received` is not batching evidence; direct batching evidence requires
  `batched` or `confirmed`.
