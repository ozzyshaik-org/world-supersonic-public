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
| Pre-kickoff documentation commits | the commits listed below (24-25 Oct 2026), plus any documentation-only commit made before the start tag | Documentation and CI only, no application code | Pre-existing |
| Event work | commits after the annotated tag `ethglobal-mumbai-2026-start` | The three features below | Hackathon work |

## Baseline (before the hackathon)

Everything reachable from the tag `baseline-pre-ethglobal-mumbai` is pre-existing work, described in the README:

- Reth ExEx that streams blocks, transactions, traces and logs over gRPC
- Rust `tank-engine` ingest into ClickHouse, with a Valkey cache
- Go `rpc-gateway` (REST `/v1`, API-key tiers, rate limits) and a Rust WebSocket edge
- Python `sgip` explorer and `mev-detector`
- Ingestion into the production database since 2026-03-15, with earlier chain history backfilled (see README)

## Pre-kickoff commits after the baseline tag (pre-existing, not hackathon work)

The commits below were made after the baseline tag and before the hackathon kickoff (seven as of 25 Oct 2026; later
documentation-only commits made before the start tag are also pre-existing). They are pre-existing work.
They touch Markdown files and one CI workflow only; no file under `cmd/`, `edge/`, `exex/`, `tank-engine/`,
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
`docs/hackathon/` are part of this pre-kickoff documentation. They describe what will be
built; the implementation is event work.

Once the start tag exists, list them with:

```bash
git log --oneline baseline-pre-ethglobal-mumbai..ethglobal-mumbai-2026-start
```

## Hackathon work (starts at kickoff, 5 Nov 2026, 9:00 PM IST)

At kickoff an annotated tag `ethglobal-mumbai-2026-start` is created on the last commit before the first event
commit and pushed. Event work is everything after that tag. History is never rewritten and the tags are never moved.

The three features, each with a definition of done:

1. **x402 pay-per-request data access** with Circle Gateway nanopayments in USDC on Arc.
2. **World ID gating**: one API key per verified human.
3. **ENS-native inputs**: ENS names accepted wherever an address is.

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
