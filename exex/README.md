# worldchain-exex

A [reth](https://github.com/paradigmxyz/reth) Execution Extension (ExEx) for the OP-Stack Worldchain node.
It runs inside the `op-reth` process, and streams each committed block (header, transactions, call traces, logs)
over gRPC on a unix socket (`/var/run/worldchain/node_stream.sock`, see `proto/nodestream.proto`) to `tank-engine`.

## Build status — read this first

* Production builds this crate against a **local checkout of reth at commit `27a8c0f`** (upstream commit
  `27a8c0f5a6dfb27dea84c5751776ecabdd069646`, 2025-11-18). The checkout used in production is a locally modified copy;
  the modifications are **not included** in this repository.
* `Cargo.toml` here replaces the private path dependencies with a plain git pin of upstream reth at that commit.
  **This has not been build-verified.** Expect to adjust crate paths/versions (reth is a large monorepo whose
  workspace pins `revm`/`alloy` versions) or to supply your own reth checkout via `[patch]`.
* `Cargo.lock` is intentionally omitted (it recorded the private path layout).
* `EXEX_BYPASS_UNDER_BLOCK` (env, default `0`): block-number threshold read at startup and logged with the ExEx head; see `src/main.rs`.
