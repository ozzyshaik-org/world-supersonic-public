# Feature 1: x402 pay-per-request access (Arc, Circle Gateway, USDC)

Status: specification. Implementation starts 5 Nov 2026.

## Goal
Let any client, human or AI agent, pay per request for World Chain data in USDC, with no account or API key.

## Design
- A payment middleware wraps the `/v1` mux in `cmd/rpc-gateway`, next to the existing API-key middleware.
- No valid `X-API-Key` and no payment header: respond `402 Payment Required` with the price, asset (USDC), network and `payTo` address.
- A client pays through Circle Gateway nanopayments on Arc and retries with the payment header.
- The gateway verifies and settles, then serves the response. Existing rate limits keep applying.
- Per-route price table: path to price in USDC.
- Project is started from Arc Studio, and the demo has a public URL.

## Done means
1. A request to a paid `/v1` route without payment returns HTTP 402 with machine-readable payment requirements.
2. A USDC payment settles on Arc and the retried request returns the data.
3. A demo client script performs the full flow in one command.
4. Open-source code, README section and a live demo URL.
