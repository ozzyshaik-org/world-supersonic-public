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

## x402 seller text-record schema (draft, project-defined; increment 2)
`parseSellerRecords(resolution, policy)` turns resolved records into a typed `SellerDescriptor` or a typed `SellerError`. It is pure and makes no network calls. **Evidence tier: unit/local only.** No ENS name carries these records yet.

| Key | Required | Rule |
|---|---|---|
| `org.x402.endpoint` | yes | Must be `https`, with no userinfo, no IP literal (IPv4 in any form, or IPv6), and not `localhost`/`*.localhost`. The host must be an exact match on the operator allowlist. A non-default port must be allowlisted for that host. |
| `org.x402.price` | yes | Positive integer string in atomic units, `1 … 2^256-1`, no leading zeros. |
| `org.x402.payTo` | yes | EIP-55 checksummed address, which **must equal the name's `addr` record**. |
| `org.x402.network` | yes | CAIP-2 id in the `eip155` namespace (`eip155:<chainId>`). |
| `url`, `description` | no | ENSIP-5 global keys, passed through. |

Vendor keys use ENSIP-5 reverse-DNS notation (`org.x402.*`). This is our own convention, not an ENS or x402 standard.

Limit: the guard checks the URL only. Whoever later fetches the endpoint must still pin resolved IPs and refuse private, loopback and link-local ranges, because DNS can change after this check. No fetch exists yet.

Planted-bug check (manual): removing any of these checks made `npm test` fail: https, userinfo, IP literal, port, checksum, payTo-addr match, uint256 bound, or exact host match (swapped for a prefix match). A redundant trailing-dot branch survived its planted bug and was removed.

This is not mainnet, not a registered name, and not wired into the seller or API yet.
