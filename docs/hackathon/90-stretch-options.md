# Stretch options (not committed)

Status: uncommitted options; specification only. Nothing here is committed scope, implemented or verified.

Official kickoff on 5 Nov 2026; any implementation would be event work after the start tag.

## Rules for these options
- Neither option below is committed.
- Only one may be attempted, and only after Arc x402 ([01](01-x402-arc-gateway.md)), World ID IDKit ([02](02-world-id-gating.md)), World ID Agent Pass ([04](04-agent-pass.md)) and ENS ([03](03-ens-integration.md)) are complete and evidenced in [EVIDENCE.md](../../EVIDENCE.md).
- No prize eligibility is claimed for either option. Sponsor terms are not copied here; any detail that has not been checked against the sponsor's official page is marked "to be rechecked".
- No sponsor other than those named in this document is considered.

## Option A: Arkiv
Idea: portable, expiring agent-query and payment receipts, with an actual user-visible Arkiv write and an actual user-visible Arkiv read.

- A receipt is written to Arkiv for a completed agent query or payment, and the user sees that write happen in the product.
- The receipt is later read back from Arkiv and shown to the user, so the user can see it was stored and retrieved, not only that a call was made.
- Receipts expire. The expiry mechanism and its exact semantics on Arkiv are to be rechecked.
- A receipt contains no sensitive request or payment data: no request body, no sensitive query content, no credentials, no payer secrets and no World ID token or `(iss, sub)`. Which fields are safe to store is decided before any write, and the set is kept minimal.
- A receipt does not replace the Arc/x402 ledger from spec 01 or any evidence required there.
- Arkiv network, SDK, prize terms and qualification requirements: to be rechecked.

## Option B: CROPS
Idea: one substantive new CROPS property implemented during the event.

- Exactly one new property is chosen, named before work starts, and implemented after the event start tag.
- Pull requests #7 and #9 are pre-existing work. They cannot count toward this option.
- The choice of property, what counts as substantive, and the evidence that it is new are to be rechecked against the final CROPS terms.
- Final CROPS terms require rechecking before any decision to attempt this option.

## Watchlist only
- Uniswap: watchlist only. It is not an option and no work is planned. It becomes eligible for consideration only after an official qualifying Uniswap integration has been verified against Uniswap's own official documentation and prize terms, which is to be rechecked.
