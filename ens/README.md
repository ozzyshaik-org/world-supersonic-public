# ens/: read-only ENSv2 name resolution (Sepolia first)

Status: **experimental, read-only, first increment.** No ENS names are registered by this project yet. Nothing here writes on-chain, holds keys or uses a wallet.

## What it does
`resolveEns(name, textKeys, client?)` normalises a name (ENSIP-15), then reads its address (`addr`, ETH coin type) and the named text records through the ENS Universal Resolver. It issues `eth_call` only. The default client is Sepolia via a public RPC (`https://ethereum-sepolia-rpc.publicnode.com`).
- Malformed names and bad text keys are rejected with `EnsInputError` before any RPC call.
- Unset records come back as `null`.
- The module never fetches any URL found in a record.

## Why Sepolia uses ENSv2 here
- docs.ens.domains, Deployments (https://docs.ens.domains/learn/deployments): "Sepolia runs the ENSv2 contracts: the Universal Resolver and the ENS apps for Sepolia resolve through this deployment."
- For App Developers (https://docs.ens.domains/ensv2/tutorial-app-developers/): "ENSv2 is currently deployed on Sepolia for testing. The Universal Resolver is an upgradeable proxy that lives at the same address on mainnet and Sepolia…" The docs also warn against hard-coding a Universal Resolver address, so we rely on viem's built-in proxy address (`0xeEeEEEeE14D718C2B47D9923Deab1335E144EeEe`).

## Tests and evidence tiers
| Command | Tier | What it shows |
|---|---|---|
| `npm test` | unit (fake JSON-RPC transport) | Request goes to the UR proxy with the EIP-137 namehash and hand-written DNS encoding, normalisation, null handling, and input rejection with zero RPC calls. Oracles are published EIP-137 vectors, not our own output. |
| `npm run test:live` | **live-testnet-read** | Real Sepolia `eth_call`s for `gregskril.eth` (addr and avatar) and `ens.eth` (no addr). Expected values were first checked by an independent raw-ABI Python `eth_call` with no ENS library. |

**Caveat:** `gregskril.eth` is resolved via the ENSv2 Universal Resolver, but the resolver it reports (`0x322b…89cb0`) is the `ENSV1Resolver` listed in the Sepolia (ENSv2) table. It is an unmigrated v1 name read through the v2 entry point, not a record stored in an ENSv2 Permissioned Resolver. A native v2 test name needs an owner-authorized registration.

Planted-bug check (manual, at commit time): skipping normalisation, skipping key validation, or allowing single-label names each makes `npm test` fail.

This is not mainnet, not a registered name, and not wired into the seller or API yet.
