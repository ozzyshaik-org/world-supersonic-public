# ENSv2 integration (dropped)

Status: **dropped, retained as pre-kickoff documentation.** This is not planned work and not a partner prize target. The committed scope is Arc x402 and World ID IDKit; see [CONTINUITY.md](../../CONTINUITY.md).

## Goal
Use ENSv2 names as first-class identity in the API and explorer, beyond cosmetic lookups.

## Design
- `/v1/balance`, `/v1/transactions` and related routes accept `name.eth` wherever an address is accepted; resolution runs on Ethereum mainnet and is cached in Valkey.
- Responses reverse-resolve addresses to primary names where one is set.
- An ENS text record of an account drives a real setting, for example the account's alert or webhook preference.
- The explorer shows ENS names for addresses.

## Done means
1. ENSv2 names resolve on Ethereum mainnet in the API and the explorer.
2. A name's record changes a real behavior.
3. Open-source code, README section and a live demo.
