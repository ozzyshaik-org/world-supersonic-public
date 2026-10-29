# Feature 2: World ID gating (one active self-service free-tier key per action-scoped nullifier)

Status: committed scope; specification only

Official kickoff on 5 Nov 2026; implementation is event work after the start tag.

## Goal
Issue self-service free-tier API keys only to humans verified with World ID, with one active self-service free-tier key per action-scoped nullifier for the key-minting action.

## Scope
This is the World ID IDKit integration. World ID Agent Pass is a separate official integration, specified in [04](04-agent-pass.md) as the third priority and not part of this spec; no Agent Pass eligibility is claimed unless its complete verification journey is built.

## Guarantee
A World ID nullifier is scoped to a user, an app and an action. The same human has a different, unlinkable nullifier in another app or for another action. The guarantee here is therefore action-scoped: one active self-service free-tier key per nullifier for this app's key-minting action. It is not a global one-human claim.

The IDKit guarantee covers self-service free-tier keys only. Keys minted by an operator with the admin token (`X-Admin-Token`) are a separate, documented exception. They carry an `operator` origin, have no verification record, are outside the guarantee, and are not used in the human-flow demo.

### Trust decision and credential choice
The trust moment is issuance of a limited free-tier API key, which is a Sybil-resistant benefit intended for one human per action-scoped nullifier. Proof of Human is the minimum sufficient credential for that decision. The flow does not request Passport/NFC or other stronger credentials because the application does not need legal identity, nationality or age assurance.

## Design
- Key minting (`POST /v1/auth/keys`) by the self-service path requires a World ID proof from IDKit, verified server-side. The signing key for the request context is held only on the server.
- Only Proof of Human proofs are accepted, and legacy proofs are rejected, so one human cannot mint once per proof generation.
- Verification and key data live in a dedicated transactional SQLite store (WAL mode, busy timeout set), written by a single writer. ClickHouse is not used for this data.
- Fail-closed rule: protected actions are refused and nothing is written if verification, the store or the key check is unavailable.

### Data model
Verification records and key records are separate tables.

| Table | Columns | Constraints |
|---|---|---|
| `verification` | `id`, `action`, `nullifier`, `created` | `UNIQUE(action, nullifier)`. Written once per action-scoped nullifier; never updated by key rotation, revocation or recovery. |
| `key` | `id`, `verification_id`, `key_hmac`, `hmac_version`, `status` (`active`, `rotated`, `revoked`), `origin` (`self-service`, `operator`), `created`, `rotated_or_revoked_at`, `replaced_by` | A partial unique index allows one row with `status = active` per `verification_id` for self-service keys. Operator keys have no `verification_id`. |
| `challenge` | `id`, `value`, `expires`, `consumed` | Single use. |

API keys are stored as an HMAC (HMAC-SHA256 with a server-held secret, versioned so it can be rotated). The plaintext key is shown once and is never logged or stored.

### Minting
Key issuance is one atomic transaction: consume the challenge, insert the verification record, insert the key. A concurrent or replayed duplicate hits the `UNIQUE(action, nullifier)` constraint and is refused with a constant error, with no second key and no partial rows.

### Rotation and revocation
Rotation with the current key revokes that key and issues a new one in one transaction. Operators can revoke a key.

### Lost-key recovery
A human who lost the key proves again with a fresh valid World ID proof bound to a new challenge (`POST /v1/auth/keys/recover`). In one transaction the service consumes the challenge, locates the existing verification record by `(action, nullifier)`, marks the old active key `revoked` and inserts the replacement key. No nullifier row is inserted. A proof whose nullifier has no verification record is refused and directed to minting. If any step fails, the transaction rolls back: the old key stays active and no replacement exists. The same proof result sent twice is refused because its challenge is consumed.

### Revocation propagation
| Surface | Rule |
|---|---|
| `/v1` | The gateway checks key status on every request. Cached key status lives at most 5 seconds, and revocation deletes the cache entry, so a revoked key is refused within 5 seconds and normally at once. |
| `/ws` new connections | The gatekeeper in `cmd/api` checks key status at connect; a revoked key is refused. |
| `/ws` open sessions | The gatekeeper re-checks the key of every open session at least every 5 seconds and closes the session of a revoked key with a policy-violation close code, so an open session ends within 10 seconds of revocation. |

The 5-second `/v1` revocation and 10-second open-`/ws` closure are tested upper-bound targets under normal operating conditions, not production SLA guarantees.

### Edge access
The guarantee holds only if every path to `/v1` and `/ws` passes the key check. Direct access to the gateway on `:8081`, to `cmd/api` on `:8090`, to the edge on `:8091` and any other proxy path is closed to the public network, or it applies the same key check. The edge accepts connections only from the gatekeeper.

## Done means
1. Successful verification: a key is minted only after a valid World ID proof, and the key works on `/v1` and `/ws`.
2. Cancellation or denial: if the user cancels or World ID denies the proof, no key is minted and no nullifier row is written.
3. Replay: sending the same proof result a second time is refused and leaves one key.
4. Concurrent duplicates: parallel mint attempts with one nullifier produce exactly one key; every other attempt is refused.
5. Rotation and revocation: rotation issues a new working key and the old key stops working; a revoked key is refused on `/v1` and `/ws`.
6. Lost-key recovery tests:
   - A fresh valid proof for an existing verification record revokes the old key and issues a working replacement.
   - The `verification` table has the same row count and the same row before and after recovery.
   - After recovery exactly one active key exists for the verification record.
   - A failure injected between revocation and issuance leaves the old key active and creates no replacement.
   - A replayed recovery proof and a proof with an unknown nullifier are refused.
7. Revocation propagation tests: a revoked key is refused on `/v1` within 5 seconds; a `/ws` session that was open at revocation is closed within 10 seconds; a new `/ws` connection with the revoked key is refused. The 5-second and 10-second limits are tested upper-bound targets under normal operating conditions, not production SLA guarantees.
8. Edge evidence: an external probe from outside the host, with its output recorded in the evidence, shows direct access to `:8081`, `:8090`, `:8091` and any other proxy path is refused or closed, or applies the same key check.
9. A protected action returns an error and writes nothing when verification or the store is unavailable.
10. The human-flow demo mints its key through the self-service IDKit path only. No operator or admin-minted key appears in it.
11. Open-source code, README section and a live demo.
12. Include a World ID integration debrief covering time to first success, friction encountered, missing capabilities or documentation, and the one improvement with the greatest impact.
