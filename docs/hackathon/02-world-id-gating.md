# Feature 2: World ID gating (one API key per human per action)

Status: committed scope; specification only

Official kickoff on 5 Nov 2026; implementation is event work after the start tag.

## Goal
Issue API keys only to humans verified with World ID, one active key per verified human for the key-minting action.

## Scope
This is the World ID IDKit integration. World Agent Pass is a separate official integration and an optional stretch, not part of this spec; no Agent Pass eligibility is claimed unless its complete verification journey is built.

## Guarantee
A World ID nullifier is scoped to a user, an app and an action. The same human has a different, unlinkable nullifier in another app or for another action. The guarantee here is therefore action-scoped: one active API key per human for this app's key-minting action. It is not a global one-human claim.

## Design
- Key minting (`POST /v1/auth/keys`) requires a World ID proof from IDKit, verified server-side. The signing key for the request context is held only on the server.
- Only Proof of Human proofs are accepted, and legacy proofs are rejected, so one human cannot mint once per proof generation.
- Nullifiers live in a dedicated transactional SQLite store (WAL mode, busy timeout set), written by a single writer. ClickHouse is not used for this data.
- The store enforces `UNIQUE(action, nullifier)`. Key issuance is one atomic transaction: consume the challenge, insert the nullifier, insert the key. A concurrent or replayed duplicate hits the constraint and is refused with a constant error, with no second key and no partial rows.
- API keys are stored as an HMAC (HMAC-SHA256 with a server-held secret). The plaintext key is shown once and is never logged or stored.
- At most one active key exists per human. Rotation with the current key revokes the old key and issues a new one in one transaction. Revocation is available to operators, and a revoked key stops working on `/v1` and `/ws`. The HMAC secret is versioned so it can be rotated.
- Protected actions fail closed: if verification, the store or the key check is unavailable, the action is refused and nothing is written.
- The WebSocket gatekeeper in `cmd/api` applies the same key check.
- Admin-token minting stays available for operators.

## Done means
1. Successful verification: a key is minted only after a valid World ID proof, and the key works on `/v1` and `/ws`.
2. Cancellation or denial: if the user cancels or World ID denies the proof, no key is minted and no nullifier row is written.
3. Replay: sending the same proof result a second time is refused and leaves one key.
4. Concurrent duplicates: parallel mint attempts with one nullifier produce exactly one key; every other attempt is refused.
5. Rotation and revocation: rotation issues a new working key and the old key stops working; a revoked key is refused on `/v1` and `/ws`.
6. A protected action returns an error and writes nothing when verification or the store is unavailable.
7. Open-source code, README section and a live demo.
