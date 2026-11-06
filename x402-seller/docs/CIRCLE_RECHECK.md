# Circle documentation re-check (2026-11-06 IST)

Re-checked against Circle developer docs while correcting draft PR #23.
This package still makes **no live** Circle / Arc Testnet calls.

## Sources consulted

| Topic | URL | Finding |
|---|---|---|
| EIP-3009 signing / validity window | https://developers.circle.com/gateway/nanopayments/howtos/eip-3009-signing | `validBefore` must be **at least 3 days** in the future or Gateway rejects with `authorization_validity_too_short`. Domain name `GatewayWalletBatched`. `verifyingContract` from `getVerifyingContract()` or `402` `accepts[].extra.verifyingContract`. Payment payload is x402 v2 with `accepted` holding the selected option. |
| SDK / settle errors | https://developers.circle.com/gateway/nanopayments/references/sdk | `SettleResponse = { success, errorReason?, payer?, transaction }`. `nonce_already_used` and `unexpected_error` listed. `TransferStatus`: received \| batched \| confirmed \| completed \| failed. |
| Settle API | https://developers.circle.com/api-reference/gateway/all/settle-x402payment | HTTP 200 body `success`; `transaction` UUID on success / empty on failure; HTTP 400 malformed; HTTP 500 `unexpected_error`. |
| Contract addresses | https://developers.circle.com/gateway/references/contract-addresses | Arc Testnet GatewayWallet: `0x0077777d7EBA4688BDeF3E311b846F25870A19B9` (sourced into `src/chainConfig.ts`). |
| Seller reconciliation | https://developers.circle.com/gateway/nanopayments/howtos/x402-seller-reconciliation | Completed request confirms acceptance, not settlement. Nonce unique per payment → at most one transfer. Wait for `confirmed` before accounting credit and `completed` before dependent onchain ops. This project is stricter: only `completed` is locally settled. |
| Batched settlement | https://developers.circle.com/gateway/nanopayments/concepts/batched-settlement | Seller submits; Gateway verifies/locks/queues; seller serves immediately; Gateway batches later. |

## Installed SDK vs online declaration

| Item | Installed (`@circle-fin/x402-batching@4.0.0`) | Online docs (2026-11-06) |
|---|---|---|
| Package | 4.0.0 (optionalDependency) | Documents `BatchFacilitatorClient` from `@circle-fin/x402-batching/server` |
| Peer deps (package.json) | `@x402/core` ^2.3.0, `@x402/evm` ^2.3.0, `viem` ^2.0.0 | README says install also pulls `@x402/core` and `viem`; `@x402/evm` “only needed if using CompositeEvmScheme or GatewayEvmScheme” |
| Import of `/server` without `@x402/evm` | **Fails** at runtime: `Cannot find package '@x402/evm'` | README understates the peer requirement for the server entry point |
| Action taken | Declare `@x402/evm` ^2.3.0 as a dependency so the server entry imports without constructing a live client | N/A |

## Explicitly UNRESOLVED (no official docs found establishing these)

1. **Nonce scope** — Circle says the nonce is unique per payment but does not say whether uniqueness is per payer, per chain, or global inside Gateway. Spec 01 keeps `auth_key = network|payer|nonce` plus a UNIQUE index on nonce alone. Not silently relaxed.
2. **Transfer amount units** — Whether `amount` on a transfer record is decimal USDC or atomic base units, and whether settle `transaction` equals the transfer `id`, remain **to be confirmed against official docs**. Matching in this package compares the stored binding `price` string to `transfer.amount` as returned by the (FAKE) lookup surface.

## Not claimed

- No live `BatchFacilitatorClient` construction
- No live settle / verify / transfer lookup against Circle or Arc Testnet
- No verified end-to-end Gateway integration
