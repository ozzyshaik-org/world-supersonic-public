# Feature 3: ENS name inputs for address parameters

Status: committed scope, fourth priority; specification only

Official kickoff on 5 Nov 2026; implementation is event work after the start tag.

## Scope order
1. Arc x402 ([01](01-x402-arc-gateway.md)): core priority.
2. World ID IDKit ([02](02-world-id-gating.md)): core priority.
3. World ID Agent Pass ([04](04-agent-pass.md)): third priority.
4. ENS name inputs for address parameters (this spec): fourth priority. It is attempted after the Arc, IDKit and Agent Pass flows work end to end, with their demos, tests and evidence complete. ENS scope does not extend beyond what this document lists.

## Goal
Let API callers pass an ENS name for the address parameter of the initial routes, `/v1/balance` and `/v1/transactions`.

## Design
- The initial ENS routes are `/v1/balance` and `/v1/transactions`. Both accept `name.eth` wherever they accept an address. No other route accepts ENS names.
- Resolution of the name to an address runs on Ethereum mainnet. Results are cached in Valkey with a short TTL.
- The resolved address is returned in the response next to the name that was supplied, so the caller can see what was queried.
- A name that does not resolve returns a `404` with a clear error; malformed names return `400`.
- Resolution makes outbound calls only to the configured Ethereum mainnet RPC endpoint. The service never fetches a URL taken from an ENS record.
- ENS text records are not read and drive no behavior.
- **Paid requests (Arc, spec 01).** The Arc request fingerprint covers the ENS name exactly as the client supplied it, in its canonical-query form, not the resolved address. It is computed before resolution, so it is stable across retries and a later change to the name's records cannot change what was authorized. Names that differ only in letter case are different requests. The resolved address, with the time and block of resolution, is stored next to the response and receipt, and a fulfilled authorization returns its stored response unchanged on retry. A `404` or `400` from resolution is not charged. See spec 01, "ENS names and the fingerprint".
- **Agent Pass containment.** Ordinary ENS support stays on these two routes. Initial Agent Pass operations ([04](04-agent-pass.md)) accept literal addresses only (`0x` followed by 40 hex characters); an ENS name in an Agent Pass proposal is refused with `400`, so approvals, fingerprints and payment bindings there never depend on resolution.

## Out of scope
- Any route other than `/v1/balance` and `/v1/transactions`.
- Reverse resolution and primary-name display in responses or the explorer.
- ENS text records and any behavior driven by them (see the security review below).
- Any other ENS feature.

## Done means
1. A request to `/v1/balance` or `/v1/transactions` that passes a `name.eth` for an address parameter resolves through Ethereum mainnet and returns the same data as the resolved address.
2. Unresolvable and malformed names return the documented errors.
3. Tests cover resolution, cache hits, and both error cases.
4. Fingerprint tests (PLANNED): a paid request with an ENS name is fingerprinted over the name as supplied and not the resolved address; two names that resolve to the same address have different fingerprints and a nonce reuse across them is refused; a change of resolution between attempts changes neither the fingerprint nor a stored response. A proposal for an Agent Pass operation that contains an ENS name is refused with `400`.
5. Open-source code, README section and a demo.

## Partner prize eligibility
No ENS partner prize eligibility is claimed for ordinary `.eth` resolution alone. A prize-targeted ENS integration must use ENSv2 on Ethereum mainnet in a functional, central, non-cosmetic way. Eligibility is stated only if the delivered integration meets that condition.

## Security review of the earlier proposal
The earlier version of this spec proposed that an ENS text record of an account would drive a real setting, for example the account's alert or webhook preference. That proposal is removed.

- Why: text records are written by whoever controls the name, which is not necessarily the caller or the service operator. A webhook URL read from a record would make the server fetch an attacker-chosen URL (server-side request forgery toward internal services, loopback, link-local and cloud metadata addresses) and would let anyone who controls a name trigger outbound calls to arbitrary hosts.
- Replacement: none. The design above reads no text records and makes no outbound call to any record-controlled URL. The only outbound call in this feature is to the configured Ethereum mainnet RPC endpoint.
- Condition for any future outbound-call feature: it would need its own reviewed spec first, with at least an operator-configured allowlist of hosts, https only, blocking of private, loopback, link-local and metadata address ranges, no redirects, resolved-IP pinning, short timeouts and response size limits.
