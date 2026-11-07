# World ID Agent Pass — experimental elevated-read checkpoint

Status: **implemented experimental branch; local fake-OIDC verification only**. No official World hosted journey, production deployment, Arc payment composition, or prize eligibility is claimed by this directory alone.

## Product event

An agent proposes an elevated Supersonic investigation read. The backend pauses the exact request. A signed-in human links a World ID, completes fresh World ID authentication, separately approves or denies the stored request, and only then can the protected read execute once. Agent Pass approval is not an Arc payment approval and this slice never calls Circle Gateway.

## Implemented boundary

- Live OIDC adapter using `openid-client` 6.8.9, official issuer discovery, authorization code flow, fresh `state` and `nonce`, S256 PKCE, `max_age=0`, and an essential Orb authentication-class request.
- Backend checks for callback issuer/session, linked `(iss, sub)`, `auth_time`, exact `acr`, and exact fresh-proof `amr=["pop"]`; passkey-only `["pop","user","mfa"]` is rejected under this initial policy.
- Separate application-owned account linking and exact-operation approval. The World ID token authenticates; it does not approve the business action.
- SQLite transactions for approval consumption and execution reservation, fixed expiry, cancellation, denial, replacement-attempt supersession, replay rejection, and stored-response read-back.
- Same-origin server-rendered approval journey with CSRF checks, a `Secure; HttpOnly; SameSite=Lax` session cookie, no credentialed CORS, no browser client secret, and no ID/access-token persistence or downstream forwarding.
- Agent API separated by its own bearer credential; callback and approval URLs use the configured public origin, never request `Host`.
- Elevated reads only. `FetchProtectedAction` performs an allowlisted server-to-server GET; `SyntheticProtectedAction` is explicitly labeled local demo data.

## Verification on this branch

| Layer | Result | Meaning |
|---|---|---|
| TypeScript | `npm run typecheck` and `npm run build` passed | Static/build verification only |
| Service/security | 19/19 tests passed | Fake OIDC and synthetic protected action; success plus adversarial state transitions |
| HTTP | 4/4 tests passed | Real local HTTP server with fake OIDC and synthetic protected action |
| Official World hosted flow | `NOT_YET_RUN` | Requires owner portal registration, credentials, browser completion, and sanitized evidence |
| Live Supersonic upstream | `NOT_YET_RUN` in this branch's verification | Local HTTP used the synthetic action |

Run the reproducible local checks with Node 20 or 22:

```bash
cd agent-pass
npm ci
npm run check
```

## Local demo setup

1. Register a confidential client in the World developer portal. Use issuer `https://auth.world.org`, the exact callback `<public-origin>/auth/world/callback`, and the registered client authentication method.
2. Copy `.env.example` to a private environment location outside the repository. Set `WORLD_CLIENT_ID`, `WORLD_CLIENT_SECRET`, a random `AGENT_PASS_DEMO_AGENT_CREDENTIAL`, and the exact public origin. Never commit or paste the secret into logs, screenshots, chat, or browser code.
3. For a synthetic local demo only, set `AGENT_PASS_ENABLE_DEMO_SESSION=1`. Without an upstream this returns data marked `synthetic: true` and `source: local-demo-only`.
4. For a real protected read, configure `AGENT_PASS_UPSTREAM_ORIGIN` separately and leave demo-session provisioning disabled in a production-shaped run. Do not expose the upstream or this service publicly without infrastructure authorization and TLS.
5. Start with `npm start`, open `/demo`, create the synthetic application session, link World ID, then propose an operation through the agent API. The response contains the approval URL.

Example proposal (replace only the local credential and account in your private shell; this is not a live World test):

```bash
curl -sS http://127.0.0.1:8097/v1/agent-pass/operations \
  -H "Authorization: Bearer $AGENT_PASS_DEMO_AGENT_CREDENTIAL" \
  -H 'Content-Type: application/json' \
  --data '{"accountId":"demo-account","method":"GET","routePath":"/v1/transactions","query":"address=0x0000000000000000000000000000000000000001&limit=1"}'
```

## Remaining evidence required before a completed-prize claim

- Official World client registration and a genuine hosted user-completed callback, validated by this backend.
- Successful and denied/expired/cancelled official journeys showing whether the protected action occurred.
- Real protected upstream result, or an explicitly synthetic demo result with no live-upstream claim.
- Sanitized screenshots/video and deployment URL, with no state, nonce, code, token, secret, pairwise subject, or sensitive query disclosure.
- Integration debrief filled from the official run, including actual time to first success.

## Integration debrief

| Heading | Current record |
|---|---|
| Chosen use case | Human checkpoint for one agent-proposed elevated Supersonic investigation read |
| Time to first official World success | `NOT_YET_RUN` |
| Integration friction | Local implementation required careful separation of OIDC authentication from application-owned action approval and assurance policy. This is implementation feedback, not a hosted-API result. |
| Missing capability or documentation | Under review during the official run; no unobserved deficiency is claimed |
| One high-impact improvement | A complete copy-pasteable confidential Node example covering the exact `claims` JSON, discovery, PKCE, callback `iss`, nonce, `auth_time`, `acr`, `amr`, and denied-path test would shorten integration. |
| What changed as a result | The implementation uses discovery, an essential class request, backend-only redemption, separate stored-action approval, and fail-closed single-use execution. |

## AI assistance

This experimental slice, its tests and this documentation were drafted with AI assistance (Kimi) at the owner's direction. The local checks above were actually run. No human code review, merge, deployment, official World result or prize outcome is claimed by this entry.

Official references: <https://auth.world.org/docs>, <https://auth.world.org/docs/guides/oidc>, and <https://auth.world.org/docs/guides/step-up>. The World prize page notes that proofs use fake identities; this integration must not be represented as production human assurance.
