# Continuity: baseline vs. hackathon work

This is the public copy of Worldchain Supersonic, a World Chain indexing and API stack that runs in
production. It is the base project for the ETHGlobal Mumbai 2026 Continuity Track (hackathon: 5-7 Nov 2026).

## About this repository's history

This public repository and its baseline tag were created on **24 Oct 2026**, before the hackathon starts,
as a sanitized snapshot of the project. The private development history, which goes back months, is not
public. The first commit here is the snapshot, not the start of the project.

## Baseline (before the hackathon)

The annotated tag `baseline-pre-ethglobal-mumbai` marks the repository exactly as it stood before any
hackathon work. Everything reachable from that tag is pre-existing work, described in the README:

- Reth ExEx that streams blocks, transactions, traces and logs over gRPC
- Rust `tank-engine` ingest into ClickHouse, with a Valkey cache
- Go `rpc-gateway` (REST `/v1`, API-key tiers, rate limits) and a Rust WebSocket edge
- Python `sgip` explorer and `mev-detector`
- Ingestion into the production database since 2026-03-15, with earlier chain history backfilled (see README)

## Hackathon work (starts 5 Nov 2026)

Every commit after the baseline tag is hackathon work. The three features, each with a definition of done:

1. **x402 pay-per-request data access** with Circle Gateway nanopayments in USDC on Arc.
2. **World ID gating**: one API key per verified human.
3. **ENS-native inputs**: ENS names accepted wherever an address is.

To see only the hackathon changes:

```bash
git log baseline-pre-ethglobal-mumbai..HEAD
git diff baseline-pre-ethglobal-mumbai..HEAD
```

## AI assistance

AI coding agents assist with implementation (code generation, refactoring, tests, review). Architecture,
data model, feature design and acceptance criteria are by the project owner.
