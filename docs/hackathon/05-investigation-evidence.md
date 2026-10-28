# Supporting specification: investigation story and evaluator verification

Status: cross-cutting presentation and reproducibility specification only. This is not a fifth sponsor integration,
and nothing here is implemented or verified. All fixture, script and UI work is event work after the start tag.

## Why this exists

Supersonic's differentiator is the connection between execution-level raw data, derived findings and an investigation
UI. A generic paid JSON response does not demonstrate that differentiator. The event presentation must therefore make
one investigation understandable, and an evaluator must be able to reproduce the public API and SGIP claims without
access to production data, credentials, a node or private infrastructure.

This specification adds presentation and verification work only. It does not authorize a new sponsor, a new detection
algorithm, a new paid route or a change to the four committed feature specifications.

## One investigation spine

The preferred live path uses existing baseline surfaces:

1. An agent pays for an existing `/v1/mev/analytics` request, optionally scoped by `block` and/or `type`.
2. A returned row supplies a `tx_hash` and is described exactly as the detector describes it: a candidate or derived
   finding, not proof of malicious intent, a guaranteed classification or a realized-profit measurement.
3. The same `tx_hash` opens the existing SGIP `/api/tx/{tx_hash}` call-trace investigation. The UI places the paid
   response, block context, payment receipt and SGIP link next to one another so their relationship is inspectable.
4. If the selected row has a `sandwich_victim_tx`, that is only another lead to inspect; it is not a claim that the
   victim relationship has been independently established.
5. A same-authorization retry returns the stored paid response without another upstream fulfillment or charge. The
   investigation does not silently re-query changing data and call it the stored response.

The exact live transaction is chosen during the event only after the real rows and graph have been inspected. No
transaction hash, finding, graph, receipt or settlement result is invented for this plan. If no suitable live row is
available, the demo says so and uses the labelled fixture for reproducibility rather than presenting a fake live find.

If this path cannot be made coherent with the existing routes, record the limitation. Do not add a new endpoint or
heuristic under this supporting specification; that would require its own reviewed event-time scope decision.

## Public evaluator fixture

Add a small, deterministic public fixture during the event with:

- the minimum raw transaction and canonical call-trace rows needed by SGIP for one clearly labelled synthetic or
  sanitized investigation;
- the corresponding derived `world_mev_activity` row used by `/v1/mev/analytics`;
- any labels or ABI-signature rows needed to make the expected graph deterministic;
- expected REST and SGIP results, expressed as stable field-level assertions rather than fragile comparison of
  unordered JSON or generated layout coordinates;
- a manifest stating the fixture version, whether it is synthetic or sanitized, exactly what it proves and what it
  does not prove, and the source/generation procedure without credentials or private infrastructure details.

Synthetic identifiers must be unmistakably non-live. Sanitized data must not be described as an unchanged real chain
record. The fixture is evidence of reproducible service wiring and API/SGIP contracts only. It is not evidence of
chain-scale ingestion, production parity, detector precision, Arc settlement, World verification or a real human
flow. Those claims require their own evidence rows.

## One-command verification

During the event, provide one documented command from a clean checkout that:

1. uses an isolated throwaway ClickHouse database and explicit local/test configuration, never the production
   database or a developer's default environment;
2. applies `schema/quickstart_schema.sql`, loads the fixture, runs the required public services and checks the paid
   route's underlying (unpaid in this fixture) `/v1/mev/analytics` response plus SGIP `/api/tx/{tx_hash}` invariants;
3. checks the cross-link: the `tx_hash` asserted in the API result is the hash whose SGIP graph is checked;
4. exercises at least one negative control, such as a missing trace or mismatched fixture hash, and proves that the
   verification fails rather than merely printing success;
5. exits non-zero on failure, cleans up only resources it created and prints no secret or production connection
   string.

The fixture command does not call `settle()`, fabricate a Gateway result, simulate a World identity or mark any
payment/identity evidence row as passed. Arc and World success paths are demonstrated separately against their real
integrations. A test double used inside a unit test remains clearly labelled and never becomes live-run evidence.

## Presentation labels

Every demo screen, README verification section and evidence entry distinguishes:

- **Live chain data:** read from the identified deployment, with measurement date and limitations.
- **Public fixture:** synthetic or sanitized local data for reproducibility.
- **Real partner run:** an observed Arc or World integration result with its own evidence.
- **Pending / unavailable:** not working or not observed; never replaced by a scripted success.

A scripted replay may control timing and navigation, but may not alter the underlying response, identity result,
payment state or expected verdict. Any mocked interface is named at the point of use.

## Done means

1. The live investigation connects one paid derived-finding response to the SGIP graph for the same transaction,
   with block context and receipt visible and candidate/limitation wording intact.
2. The public fixture contains the required raw and derived rows, expected results and an honest provenance/limitations
   manifest, with no production credentials or private data.
3. One documented clean-checkout command verifies the REST/SGIP contract and cross-link in isolation, without a node
   or production access, and returns a meaningful failure exit status.
4. A negative control makes that command fail; live, fixture, real-partner and pending states are labelled distinctly
   in the demo and documentation.
