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
| Pre-kickoff documentation commits | the commits listed below (24-25 Oct 2026), plus any other commit made before the start tag | The listed commits are documentation and CI only; later pre-start commits are recorded here as they happen | Pre-existing |
| Event work | commits after the annotated tag `ethglobal-mumbai-2026-start` | The committed features below | Hackathon work |

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

Every commit that will be reachable from the tag `ethglobal-mumbai-2026-start` (not yet created; see below) is pre-existing. The feature specs in
`docs/hackathon/` are part of this pre-kickoff documentation. They are committed scope—specifications only, not yet implemented. They describe what will be
built; the implementation is event work.

Once the start tag exists, list them with:

```bash
git log --oneline baseline-pre-ethglobal-mumbai..ethglobal-mumbai-2026-start
```

## Hackathon work (official kickoff on 5 Nov 2026)

Immediately before event implementation begins, the then-current `main` is tagged with the annotated tag
`ethglobal-mumbai-2026-start`, the tag is pushed, and its commit hash is recorded in `HACKATHON_LOG.md`. The tag does not
exist yet. Event work is everything after that tag. History is never rewritten and the tags are never moved.

The committed scope—specifications only, not yet implemented—is three features, each with a definition of done, in scope order:

1. **x402 pay-per-request data access** with Circle Gateway nanopayments in USDC on Arc (core priority).
2. **World ID gating with IDKit**: one active self-service free-tier key per action-scoped nullifier (core priority).
3. **ENS name inputs for `/v1/balance` and `/v1/transactions`** (third priority): attempted after the Arc and IDKit flows work end to end, with their demos, tests and evidence complete. See `docs/hackathon/03-ens-integration.md`.

Optional stretch, not committed: World Agent Pass, a separate official integration, never conflated with IDKit. No eligibility is claimed unless its
complete verification journey is built.

Partner prize targets:

| Track | Priority | Eligibility |
|---|---|---|
| Arc: Bring Your Project to Arc (current prize target) | 1, core | Defined by the done criteria in `docs/hackathon/01-x402-arc-gateway.md` |
| World: Best IDKit use case (current prize target) | 2, core | Defined by the done criteria in `docs/hackathon/02-world-id-gating.md` |
| ENS (conditional prize consideration) | 3 | Third priority; no prize claim. Eligibility only if ENSv2 on Ethereum mainnet is used in a functional, central way |

To see only the event changes:

```bash
git log ethglobal-mumbai-2026-start..HEAD
git diff ethglobal-mumbai-2026-start..HEAD
```

To see everything added since the baseline snapshot (pre-kickoff documentation plus event work):

```bash
git diff baseline-pre-ethglobal-mumbai..HEAD
```

## AI assistance

AI coding agents assist with implementation (code generation, refactoring, tests, review). Architecture,
data model, feature design and acceptance criteria are by the project owner.
