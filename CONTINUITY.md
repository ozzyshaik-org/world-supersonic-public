# Continuity: baseline vs. hackathon work

This is the public copy of Worldchain Supersonic, a World Chain indexing and API stack that runs in
production. It is the base project for the ETHGlobal Mumbai 2026 Continuity Track (hackathon: 5-7 Nov 2026).

## About this repository's history

This public repository and its baseline tag were created on **24 Oct 2026**, before the hackathon starts,
as a sanitized snapshot of the project. The private development history, which goes back months, is not
public. The first commit here is the snapshot, not the start of the project.

## Layers of history

| Layer | Reference | What it contains | Status |
|---|---|---|---|
| Private development history | not public | Months of development of the full stack | Pre-existing |
| Baseline snapshot | annotated tag `baseline-pre-ethglobal-mumbai` (commit `241af1f`, 24 Oct 2026) | Sanitized public snapshot of the full stack | Pre-existing |
| Pre-kickoff commits | the commits listed below (24-25 Oct 2026), plus any other commit made before the start tag | The listed commits are documentation and CI only; later pre-start commits also include dependency, build, schema, CI, product/presentation (`website/`) and existing-core code changes and are recorded in the ledger below | Pre-existing |
| Event work | commits after the annotated tag `ethglobal-mumbai-2026-start` (tag object `7993c080fa47d3f15e584795fcb4bd59c0250012`, peeled commit `cb39dbb3d2dbf217949ef279c7e526c543c0b7ea`); under Corrected Variant B, merge commit `cb39dbb` is event-period administrative/documentation from PR #21, not an event-built feature | The committed features below, plus the administrative merge noted under Corrected Variant B | Hackathon work |

## Baseline (before the hackathon)

Everything reachable from the tag `baseline-pre-ethglobal-mumbai` is pre-existing work, described in the README:

- Reth ExEx that streams blocks, transactions, traces and logs over gRPC
- Rust `tank-engine` ingest into ClickHouse, with a Valkey cache
- Go `rpc-gateway` (REST `/v1`, API-key tiers, rate limits) and a Rust WebSocket edge
- Python `sgip` explorer and `mev-detector`
- Ingestion into the production database since 2026-03-15, with earlier chain history backfilled (see README)

## Pre-kickoff commits after the baseline tag (pre-existing, not hackathon work)

The commits below were made after the baseline tag and before the hackathon kickoff (seven as of 25 Oct 2026; any later
commit made before the start tag, including an urgent security fix, is also pre-existing). They are pre-existing work.
The seven listed commits touch Markdown files and one CI workflow only; no file under `cmd/`, `edge/`, `exex/`, `tank-engine/`,
`sgip/`, `mev-detector/`, `schema/`, `proto/` or `web/` changed.

| Commit | Date (IST) | Author | Subject |
|---|---|---|---|
| `316bc49` | 2026-10-24 22:42 | ozzyshaik | Add CONTINUITY.md: baseline vs hackathon work |
| `22c6c8f` | 2026-10-24 23:08 | ozzyshaik | Docs: badges, hackathon section, known-limits heading, SECURITY.md, issue template |
| `477a535` | 2026-10-24 23:33 | ozzyshaik | README: partner prize targets with definitions of done |
| `88a72b6` | 2026-10-24 23:44 | ozzyshaik | Docs: hackathon feature specs, hackathon log template, CONTRIBUTING, CODE_OF_CONDUCT |
| `6895c50` | 2026-10-25 00:07 | nasiparveen006 | Update README.md |
| `c22152a` | 2026-10-25 10:08 | ozzyshaik | README: link World ID and ENS feature specs |
| `5419782` | 2026-10-25 10:39 | ozzyshaik | ci: build and vet Go services |

### Pre-start pull requests merged after these commits

After `5419782`, every change reached `main` through one of the pull requests below, each merged with a merge commit
(first-parent history of `main`, in merge order). Unlike the seven commits above, several of them change code, build,
schema, dependency or CI files. Open or unmerged pull requests are not listed. The catch-all rule below still applies:
any later commit reachable from the start tag is pre-existing too.

| PR | Merge commit | Merged (IST) | Scope |
|---|---|---|---|
| #3 | `3c2701d` | 2026-10-26 09:58 | Docs: Continuity baseline correction (`CONTINUITY.md`, `HACKATHON_LOG.md`, `README.md`) |
| #4 | `b3d457b` | 2026-10-26 10:46 | Docs: specs 01–03 aligned with the selected scope (`docs/hackathon/`, `CONTINUITY.md`, `README.md`) |
| #5 | `3eb2cec` | 2026-10-26 16:01 | Docs: new `AI_USAGE.md`, `EVIDENCE.md` and `DEMO_PLAN.md`; spec 01 and README edits |
| #6 | `a1c0dc1` | 2026-10-27 19:00 | Docs: baseline readiness (`README.md`, `AI_USAGE.md`, `exex/README.md`) |
| #7 | `df16b16` | 2026-10-27 19:48 | Build: committed `exex/Cargo.lock` (locked ExEx build); `exex/Cargo.toml` comment, `exex/README.md`, `README.md` |
| #9 | `a94e39a` | 2026-10-27 20:19 | Schema: new `schema/quickstart_schema.sql` and `schema/README.md`; `README.md` |
| #10 | `0972802` | 2026-10-27 21:31 | Docs: Agent Pass spec, stretch options and priority reorder (`docs/hackathon/`, `AI_USAGE.md`, `CONTINUITY.md`, `DEMO_PLAN.md`, `EVIDENCE.md`, `README.md`) |
| #11 | `c3d58a5` | 2026-10-28 00:43 | Docs: specs 01, 03 and 04 hardened; `EVIDENCE.md` expanded; submission checklist and AI artifact plan added |
| #12 | `a0a3a35` | 2026-10-28 10:08 | Docs: investigation evidence spec (`docs/hackathon/05-investigation-evidence.md`) with `EVIDENCE.md`, `DEMO_PLAN.md`, `AI_USAGE.md` and `README.md` edits |
| #13 | `30b9caa` | 2026-10-28 19:01 | Docs: product-led `README.md` |
| #1 | `456e007` | 2026-10-29 16:52 | Dependency: `go.opentelemetry.io/otel` 1.40.0 → 1.42.0 (`go.mod`, `go.sum`) |
| #2 | `1f0dbd5` | 2026-10-29 17:10 | Dependency: `github.com/klauspost/compress` 1.18.3 → 1.18.7 (`go.mod`, `go.sum`) |
| #8 | `11394ee` | 2026-10-29 17:10 | Dependency: `rustls-webpki` 0.103.4 → 0.103.16 (`exex/Cargo.lock`) |
| #14 | `c150a6c` | 2026-10-29 17:47 | Product website: new `website/`; CI `website` job (`.github/workflows/ci.yml`); `AI_USAGE.md`, `CONTINUITY.md`, `README.md` |
| #15 | `8bf5769` | 2026-10-29 21:06 | Dependency: `rustls` 0.23.31 → 0.23.46 (`exex/Cargo.lock`) |
| #16 | `e47256b` | 2026-10-30 15:08 | CI: GitHub Pages deployment of the website (`.github/workflows/deploy-website.yml`) |
| #17 | `2ed92ef` | 2026-10-30 16:43 | Docs: website URL recorded (`README.md`, `website/README.md`) |
| #18 | `39e6c86` | 2026-11-05 01:50 | Existing-core fixes: API-key sync and `/ws` fan-out (`cmd/api`), address normalization (`cmd/api`, `cmd/rpc-gateway`), bounded ExEx DB-receipt wait (`exex/src/main.rs`), `go test` in CI; tests and docs |
| #19 | `90d1108` | 2026-11-05 12:53 | Existing-core fixes: `/ws` Gatekeeper fails closed with throttled Valkey client retries (`cmd/api`); API-key lookups with `FINAL`, a 10 s key cache and 503 for failed lookups (`cmd/rpc-gateway`); tests and docs |
| #20 | `56ad09a` | 2026-11-05 13:07 | CI: `redis-server` installed in the `go` job and Go tests run with `-v -count=1` (`.github/workflows/ci.yml`) |

### Corrected Variant B (PR #21 and the start tag)

Official hacking start: **2026-11-05 21:00:00 IST**.

Annotated start tag `ethglobal-mumbai-2026-start` exists on origin:
- Tag object: `7993c080fa47d3f15e584795fcb4bd59c0250012`
- Peeled commit: `cb39dbb3d2dbf217949ef279c7e526c543c0b7ea`
- The tag must never be moved or backdated.

**Corrected Variant B** applies:
- PR #21's seven branch commits, `c65f36f` through `e4b6346`, were made before hacking began and remain pre-existing work (documentation and website wording; not listed as event-built features).
- Only the merge commit `cb39dbb` (merged 2026-11-05 21:17 IST) is event-period administrative/documentation work from PR #21.
- No part of PR #21 is claimed as an event-built feature.
- Every other commit reachable from the start tag is pre-existing work.
- Commits after the start tag are event work, subject to their actual content and timestamps.

Every commit reachable from the tag `ethglobal-mumbai-2026-start`, other than the special case for merge commit `cb39dbb` under Corrected Variant B above, is pre-existing. The feature specs in
`docs/hackathon/` are part of this pre-kickoff documentation. They are committed scope—specifications only, not yet implemented. They describe what will be
built; the implementation is event work.

`website/` and its documentation are pre-kickoff product/presentation work when committed before the start tag. This frontend-only product site is not the planned Arc Studio event frontend, does not implement the feature specifications, and supplies no partner-integration evidence. Any event frontend or integration implemented after the start tag remains event work and must be committed separately and incrementally.

Once the start tag exists, list them with:

```bash
git log --oneline baseline-pre-ethglobal-mumbai..ethglobal-mumbai-2026-start
```

## Hackathon work (official kickoff on 5 Nov 2026)

Official hacking start: **2026-11-05 21:00:00 IST**. The annotated tag `ethglobal-mumbai-2026-start` (tag object
`7993c080fa47d3f15e584795fcb4bd59c0250012`, peeled commit `cb39dbb3d2dbf217949ef279c7e526c543c0b7ea`) is on origin and is
recorded in `HACKATHON_LOG.md`. The tag must never be moved or backdated. History is never rewritten. Under Corrected
Variant B, merge commit `cb39dbb` is event-period administrative/documentation from PR #21; commits after the start tag
are event work, subject to their actual content and timestamps.

The committed specifications remain four features, each with a definition of done, in scope order (specifications only until implemented):

1. **x402 pay-per-request data access** with Circle Gateway nanopayments in USDC on Arc (core priority).
2. **World ID gating with IDKit**: one active self-service free-tier key per action-scoped nullifier (core priority).
3. **World ID Agent Pass: human checkpoint for one paid or elevated agent data request** (third priority): attempted after the Arc and IDKit flows are complete end to end, with their demos, tests and evidence. See [`docs/hackathon/04-agent-pass.md`](docs/hackathon/04-agent-pass.md). Agent Pass approval never replaces Arc/x402 payment.
4. **ENS name inputs for `/v1/balance` and `/v1/transactions`** (fourth priority): attempted after the Arc, IDKit and Agent Pass flows are complete and evidenced. See [`docs/hackathon/03-ens-integration.md`](docs/hackathon/03-ens-integration.md).

### Frozen submission slice (6 Nov 2026)

Event implementation is **frozen** to one vertical slice until that slice and its evidence are complete:

**Arc Studio UI → real HTTP seller → Circle Gateway / Arc Testnet → existing Go API `/v1/mev/analytics` → stored response/receipt → SGIP graph for the same investigated World Chain `tx_hash`.**

Rules for this freeze:

- The **Arc payment** transaction/transfer ID is always **separate** from the investigated World Chain `tx_hash`.
- Draft PR [#23](https://github.com/ozzyshaik-org/world-supersonic-public/pull/23) head `f79de2f36b8a07371b072398ea1e2657775c3a2f` is an **experimental seller-engine checkpoint** only (unit-fake / contract-real-local).
- Draft PR [#25](https://github.com/ozzyshaik-org/world-supersonic-public/pull/25) head `b03d41ee7831dedc356590fd9170e5fec82f7afd` adds a thin **local HTTP adapter** with `local-http-integration` + unit-fake Gateway evidence (137/137). It is still not live Arc/Circle, not Studio, not a durable store, and not a public demo.
- **IDKit, Agent Pass, ENS, other partners, and pstack formal verification** are **deferred, not started**, until the functional paid `/v1/mev/analytics` flow and its evidence are complete.
- Do not present `/v1/balance` as a wallet balance.
- After PR [#22](https://github.com/ozzyshaik-org/world-supersonic-public/pull/22) merges, further docs land by normal incremental commits/merges only (no history rewrite, no force-push, no tag moves, no replacement PRs).

Arkiv and CROPS are uncommitted stretch options, documented in `docs/hackathon/90-stretch-options.md`. They are not part of the committed scope. Only one may be attempted, and only after all four committed flows are complete and evidenced. Uniswap is watchlist-only and is not planned work.

Partner prize targets:

| Track | Priority | Eligibility |
|---|---|---|
| Arc: Bring Your Project to Arc | 1, core | Defined by the done criteria in `docs/hackathon/01-x402-arc-gateway.md` |
| World: [Cont] Best IDKit Use Case | 2, core | Continuity-only listing, target pending confirmation; defined by the done criteria in `docs/hackathon/02-world-id-gating.md` |
| World ID Agent Pass | 3, targeted | Continuity-only listing, target pending confirmation; the separate general listing is not claimed; defined by the done criteria in `docs/hackathon/04-agent-pass.md` |
| ENS | 4, conditional | No prize claim. Ordinary `.eth` resolution alone supports no ENSv2 prize claim; eligibility only if ENSv2 on Ethereum mainnet is used in a functional, central way (see [`docs/hackathon/03-ens-integration.md`](docs/hackathon/03-ens-integration.md)) |

No selection, eligibility or winnings are claimed for any row. Final prize terms must be rechecked.

To see only the event changes:

```bash
git log ethglobal-mumbai-2026-start..HEAD
git diff ethglobal-mumbai-2026-start..HEAD
```

To see everything added since the baseline snapshot (pre-kickoff changes plus event work):

```bash
git diff baseline-pre-ethglobal-mumbai..HEAD
```

## AI assistance

AI coding agents assist with implementation (code generation, refactoring, tests, review). Architecture,
data model, feature design and acceptance criteria are by the project owner.
