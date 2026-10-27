# Worldchain Supersonic

[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
![Rust](https://img.shields.io/badge/Rust-ExEx%20%7C%20tank--engine%20%7C%20edge-orange)
![Go](https://img.shields.io/badge/Go-rpc--gateway%20%7C%20api-00ADD8)
![ClickHouse](https://img.shields.io/badge/ClickHouse-storage-FFCC00)
![World Chain](https://img.shields.io/badge/World%20Chain-chain%20id%20480-black)

Real-time indexing and API infrastructure for **World Chain** (OP-Stack L2, chain id 480).
It taps a reth node in-process, writes every block, transaction, call trace and log into ClickHouse,
derives DeFi / token / MEV views from them, and serves the result over REST and WebSocket.

> **How this was built.** The architecture, data model and operational decisions are by the project owner.
> AI coding agents assisted with implementation (code generation, refactoring, tests and review) under that
> human direction. Treat the code as you would any fast-moving, partly machine-written codebase: read it before you rely on it.

## Why Supersonic

Supersonic is an integrated stack for working with World Chain at the execution level, from the node to an investigation UI, on infrastructure you operate. In this repository:

* **Execution-level indexing.** `worldchain-exex` runs as a reth Execution Extension inside `op-reth` and streams each committed block (header, transactions, call traces, logs) to `tank-engine` over gRPC on a unix socket.
* **Raw data in a database you control.** Blocks, transactions, call traces and logs are written to your own ClickHouse (`world_blocks`, `world_transactions`, `world_traces`, `world_logs`), so you can query them directly with SQL.
* **Derived datasets.** `tank-engine` derives token transfers, DEX swaps and DeFi events from the raw data, and `mev-detector` adds MEV rows (priority bribes, bot spam, atomic arbitrage and sandwich candidates). `mev-detector` runs as a separate process over blocks that are already fully ingested, in chunks, so MEV rows trail the chain head.
* **REST and WebSocket access.** `rpc-gateway` serves `/v1/*` REST (see [`docs/openapi.yaml`](docs/openapi.yaml)) with API-key tiers and rate limits; `cmd/api` serves a key-gated `/ws` stream.
* **Investigation graphs (SGIP).** `sgip` renders transaction call-trace graphs and contract and wallet views from the indexed data, and renders a contract's control-flow graph (CFG) when one is stored for it. The decompiler that produces stored CFGs is not part of this repository.
* **One workflow.** Extraction (ExEx) → indexed datasets (ClickHouse) → APIs (REST/WebSocket) → investigation UI (SGIP).

Dated scale measurements and the known limits (including that the ExEx build is not yet reproducible from this repository) are under [What is live and what is not](#what-is-live-and-what-is-not-known-limits).

## ETHGlobal Mumbai 2026 (Continuity Track)

This project is the base for the Continuity Track at ETHGlobal Mumbai (5-7 Nov 2026). Pre-existing work is the annotated
tag `baseline-pre-ethglobal-mumbai` plus the commits made before the start tag (documentation and CI so far). Event work is everything after
the annotated tag `ethglobal-mumbai-2026-start`, which is created immediately before event implementation begins. See [CONTINUITY.md](CONTINUITY.md) for the full split.

Supporting documents (pre-start documentation; templates and plans, not results): [AI_USAGE.md](AI_USAGE.md) (baseline AI usage and human contributions; UI/media asset inventory remains pending before submission), [EVIDENCE.md](EVIDENCE.md) (evidence register, nothing run yet) and [DEMO_PLAN.md](DEMO_PLAN.md) (demo plan, not an existing demo).

Planned during the hackathon (committed scope—specifications only, not yet implemented):

1. **x402 pay-per-request data access** with Circle Gateway nanopayments in USDC on Arc.
2. **World ID gating with IDKit**: one active self-service free-tier key per action-scoped nullifier.
3. **ENS name inputs for address parameters**, third priority: attempted after the Arc and IDKit flows work end to end, with their demos, tests and evidence complete ([spec](docs/hackathon/03-ens-integration.md)).

Arc x402 and World ID IDKit are the core priorities. World Agent Pass is a separate official integration and an optional stretch, never conflated with IDKit.

### Partner prize targets (Continuity)

Arc and World are the current prize targets. ENS is a conditional prize consideration. Each feature has a definition of done. All new work is committed to this public repository.

| Track | Feature (spec) | Done means |
|---|---|---|
| Arc: Bring Your Project to Arc | [x402](docs/hackathon/01-x402-arc-gateway.md) pay-per-request access, paid in USDC through Circle Gateway nanopayments, started from Arc Studio | A request to a paid `/v1` route returns HTTP 402; the client retries with a signed payment authorization; the seller accepts it and fulfills the request; a retry of the same request is never charged twice and nonce reuse for a different request is rejected; paid routes cannot be bypassed through direct access; fulfilled authorizations are later settled in batch through Circle Gateway on Arc. Per-request onchain settlement is not claimed. Arc Studio URL, Studio shown in the demo, an Arc Studio-built web frontend and the seller backend, architecture diagram, integration feedback, demo URL and open-source code. |
| World: Best IDKit use case | [One active self-service free-tier key per action-scoped nullifier](docs/hackathon/02-world-id-gating.md) with World ID | A self-service key is minted only after a World ID proof, a second self-service key for the same action-scoped nullifier is refused, lost-key recovery with a fresh proof replaces the key without a new nullifier, and a revoked key is refused on `/v1` and `/ws`. Operator-minted keys are a separate documented exception and are not part of the human-flow demo. |
| ENS (conditional prize consideration) | [ENS name inputs for `/v1/balance` and `/v1/transactions`](docs/hackathon/03-ens-integration.md) | Third priority; no prize claim. Eligibility only if ENSv2 on Ethereum mainnet is used in a functional, central way; none is claimed for ordinary `.eth` resolution alone. Done criteria are in the spec. |

## Architecture

```mermaid
flowchart LR
    subgraph Node["World Chain node (OP Stack)"]
        OPNODE[op-node] --- RETH["op-reth + worldchain-exex"]
    end
    RETH -- "gRPC over unix socket<br/>(proto/nodestream.proto)" --> TANK["tank-engine<br/>ingest, healers, view matcher"]
    TANK --> CH[("ClickHouse<br/>world_* raw tables<br/>api_* derived tables")]
    CH --> GW["rpc-gateway :8081<br/>REST /v1/*, API keys, rate limits"]
    CH --> API["supersonic-api :8090<br/>WebSocket /ws (key-gated)"]
    CH --> SGIP["sgip :8095<br/>explorer / CFG visualizer"]
    CH <--> MEV["mev-detector"]
    GW <--> VK[(Valkey cache)]
    API <--> VK
    API -- "unix socket" --> EDGE["edge :8091<br/>Rust WebSocket fan-out"]
    EDGE --> CLIENTS((WS clients))
    GW --> REST((REST clients))
```

## Services

| Directory | Language | Port | What it does | State on 2026-10-24 |
|---|---|---|---|---|
| `exex/` | Rust (reth ExEx) | unix socket | Streams committed blocks (header, txs, traces, logs) out of `op-reth` | Running in production. **Build not reproducible from this repo** (private reth checkout, see `exex/README.md`) |
| `tank-engine/` | Rust | – | Consumes the ExEx stream, writes ClickHouse, backfills and heals gaps, matches logs into derived `api_*` tables | Running in production (`cargo check` passes) |
| `cmd/rpc-gateway/` | Go | 8081 | REST API `/v1/*`, Valkey cache + singleflight, API-key tiers and rate limits, Prometheus `/metrics`, Swagger `/docs` | Running; `go build`/`go vet` pass |
| `cmd/api/` + `web/` | Go | 8090 | Key-gated WebSocket `/ws` that polls recent DEX/DeFi/NFT rows and pushes them to the edge; serves a static page | Running; `go build`/`go vet` pass |
| `edge/` | Rust (axum) | 8091 | WebSocket fan-out of messages received from supersonic-api over a unix socket | Running (`cargo check` passes) |
| `sgip/` | Python (FastAPI) | 8095 | Contract / transaction / wallet explorer and control-flow-graph visualizer over ClickHouse | Running |
| `mev-detector/` | Python | – | Scans indexed blocks for MEV patterns into `world_mev_activity`; optional Telegram/Discord alerts | Running |
| `schema/` | SQL | – | ClickHouse DDL (tables, materialized views) | Reference dumps |
| `proto/` | protobuf | – | ExEx → tank-engine stream definition | – |
| `docs/openapi.yaml` | OpenAPI 3.0 | – | REST surface of the gateway | – |

## What is live and what is not (known limits)

Measured read-only against the production deployment on 2026-10-24 (approximate figures, rounded):

* The indexer was **at chain head** (indexed block ≈ 36.7 M, ingestion lag 0 blocks, node not syncing).
* ClickHouse held nearly 30 billion rows across ~55 tables: ≈ 36.7 M blocks, ≈ 720 M transactions,
  ≈ 6.9 B logs, ≈ 17.6 B call traces, ≈ 2.1 B token transfers, ≈ 177 M DEX swaps, ≈ 1.1 M MEV rows.
* Ingestion into this database started **2026-03-15** (earliest ingest timestamp); chain history before that was backfilled.

Not live / not finished — said plainly:

* **World ID classification** (`WORLD_ID_VERIFIED` in `tank-engine/src/mv_matcher.rs`, `api_humanity_feed` view) is implemented,
  but the production tables had **no matching rows** on 2026-10-24. Treat it as untested against live data.
* `POST /v1/auth/keys` was an open endpoint in production at that date. In **this** repository it requires `X-Admin-Token`
  (env `ADMIN_TOKEN`) and is disabled when the variable is unset.
* The gateway's embedded OpenAPI spec lists 5 REST routes; the code registers more (`/v1/defi/events`, `/v1/mev/analytics`,
  `/v1/portfolio`, `/v1/health/system`, `/v1/auth/keys`). `docs/openapi.yaml` documents all of them.
* `edge` `/ws` has no authentication; `cmd/api` fails open on key checks if Valkey is unavailable.
* CORS is `*` on the gateway and sgip.
* Single-host deployment, no high-availability story, and no automated test suite for the Go services.
* An older Go ingester (`internal/tank`) was superseded by `tank-engine` and is not included.
* Deployment files (systemd units, reverse-proxy config, monitoring, node genesis/rollup configs) are intentionally **not** published.

## Quick start (local, partial; these steps were compile-checked but not run end to end)

Requirements: Go ≥ 1.25, Rust stable, ClickHouse, Valkey/Redis.

```bash
cp .env.example .env            # fill in; never commit
# schema (see schema/; complete_schema.sql is a raw SHOW CREATE dump; quick-start steps are untested)
clickhouse-client --multiquery < schema/01_core_schema.sql
clickhouse-client --multiquery < schema/02_materialized_views.sql

go build -o bin/rpc-gateway ./cmd/rpc-gateway && ./bin/rpc-gateway     # :8081
go build -o bin/api ./cmd/api && ./bin/api                             # :8090
(cd edge && cargo run --release)                                       # :8091
(cd tank-engine && cargo run --release)                                # needs the exex stream socket
(cd sgip && pip install fastapi uvicorn clickhouse-connect psutil && uvicorn main:app --port 8095)
python3 mev-detector/mev_detector.py
```

Without a node running the ExEx there is no data source for `tank-engine`; you can still load your own data into the
`world_*` tables to exercise the APIs.

Create an API key (admin only):

```bash
curl -X POST localhost:8081/v1/auth/keys -H "X-Admin-Token: $ADMIN_TOKEN" \
     -H 'Content-Type: application/json' -d '{"client_name":"demo","tier":"free"}'
```

Tiers: free 2 rps, pro 20 rps, enterprise 100 rps; anonymous callers get 1 rps (burst 5). Send the key as `X-API-Key` or `?api_key=`.

## Security notes

* Credentials come from environment variables only (`.env.example`). Rotate anything that was ever committed to a predecessor repository.
* Run ClickHouse with a password and bind it to localhost.
* Put TLS and a reverse proxy in front of the public ports.

## License

MIT — see [LICENSE](LICENSE).

## Team

- [@ozzyshaik](https://github.com/ozzyshaik): architecture, backend and infrastructure
- [@nasiparveen006](https://github.com/nasiparveen006): business development and product
