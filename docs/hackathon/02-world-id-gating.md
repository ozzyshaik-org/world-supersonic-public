# Feature 2: World ID gating (one API key per human)

Status: specification. Implementation starts 5 Nov 2026.

## Goal
Issue API keys only to verified humans, one key per human.

## Design
- Key minting (`POST /v1/auth/keys`) requires a World ID proof from IDKit, verified server-side.
- The proof's nullifier hash is stored in a ClickHouse table; a repeated nullifier is refused, which enforces one key per human.
- The WebSocket gatekeeper in `cmd/api` applies the same key check.
- Admin-token minting stays available for operators.

## Done means
1. A key is minted only after a valid World ID proof.
2. A second mint attempt by the same human is refused.
3. The minted key works on `/v1` and `/ws`.
4. Open-source code, README section and a live demo.
