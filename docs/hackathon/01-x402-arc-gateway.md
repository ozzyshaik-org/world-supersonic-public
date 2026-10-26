# Feature 1: x402 pay-per-request access (Arc, Circle Gateway, USDC)

Status: committed scope; specification only

Official kickoff on 5 Nov 2026; implementation is event work after the start tag.

## Goal
Let any client, human or AI agent, pay per request for World Chain data in USDC, with no account or API key.

## Design
- A TypeScript x402 seller service, created with Arc Studio (studio.arc.io) and deployed on Arc Testnet, sits in front of the existing Go API.
- The Go API and `cmd/rpc-gateway` are unchanged. The seller service calls the Go API over HTTP with one server-side API key held in its environment, and calls no other upstream.
- Seller flow:
  1. A client requests a paid `/v1` route with no payment. The seller responds `402 Payment Required` with machine-readable payment requirements: price, asset (USDC), network (Arc Testnet) and `payTo` address.
  2. The client signs a USDC payment authorization (Circle Gateway nanopayments) and retries the request with the payment header.
  3. The seller verifies the signed authorization with Circle Gateway. On acceptance it records the authorization in a payments ledger, keyed by the authorization nonce, and fulfills the request by calling the Go API.
  4. Accepted authorizations are settled later in batch through Circle Gateway on Arc.
- A request that carries a nonce already in the ledger does not cause a second upstream call. A failed or rejected verification is never fulfilled.
- A per-payer rate limit stops one funded wallet from flooding the Go API.
- Per-route price table: path to price in USDC.
- Per-request onchain settlement is not claimed. The ledger separates an accepted authorization from a batch settlement that has reached Arc.
- The demo has a public URL.

## Done means
1. A request to a paid `/v1` route without payment returns HTTP 402 with machine-readable payment requirements.
2. The client retries with a signed payment authorization; the seller verifies it and fulfills the request; accepted authorizations are later settled in batch through Circle Gateway on Arc.
3. The Arc Studio project, created with Arc Studio, is the starting point of the seller service.
4. A request with an invalid or already-used authorization is not fulfilled and does not reach the Go API.
5. A demo client script performs the full flow in one command.
6. Open-source code, README section and a live demo URL.
