# Feature 1: x402 pay-per-request access (Arc, Circle Gateway, USDC)

Status: committed feature, specification. Official kickoff on 5 Nov 2026; implementation is event work after the start tag.

## Goal
Let any client, human or AI agent, pay per request for World Chain data in USDC, with no account or API key.

## Design
- A payment middleware wraps the `/v1` mux in `cmd/rpc-gateway`, next to the existing API-key middleware.
- No valid `X-API-Key` and no payment header: respond `402 Payment Required` with the price, asset (USDC), network and `payTo` address.
- A client signs a USDC payment authorization (Circle Gateway nanopayments, Arc) and retries with the payment header.
- The gateway verifies the signed authorization, accepts it and serves the response. Existing rate limits keep applying.
- Accepted authorizations are settled later in batch through Circle Gateway on Arc. Per-request onchain settlement is not claimed.
- Per-route price table: path to price in USDC.
- Project is started from Arc Studio, and the demo has a public URL.

## Done means
1. A request to a paid `/v1` route without payment returns HTTP 402 with machine-readable payment requirements.
2. The client retries with a signed payment authorization; the seller accepts it and fulfills the request; accepted authorizations are later settled in batch through Circle Gateway on Arc.
3. A demo client script performs the full flow in one command.
4. Open-source code, README section and a live demo URL.
