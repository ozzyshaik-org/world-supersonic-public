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
* **Build attempt on 2026-10-27 (clean clone, Debian 13, Rust 1.99.1): `cargo build --release` fails.** What was observed, in order:
  1. `reth-mdbx-sys` needs libclang (`LIBCLANG_PATH`); without it the build stops there.
  2. `tikv-jemalloc-sys` needs `make`.
  3. With those two in place, the missing lockfile lets cargo resolve `alloy-sol-macro` 1.7.3 against the pinned `alloy-sol-types` 1.4.1, and `alloy-provider 1.0.41` fails to compile.
  4. After that pin, the build still failed in `alloy-provider 1.0.41`, with `alloy-rpc-client 1.8.3` and `reqwest` 0.12.28/0.13.5 in the resolved dependency graph. The remaining compiler diagnostics have not been reduced to a verified compatible dependency set.
  The attempt stopped there. No successful build has been reproduced from this repository, and no lockfile is published. A build-verified lockfile or a `[patch]` set is the likely fix and is not available here.
* `EXEX_BYPASS_UNDER_BLOCK` (env, default `0`): block-number threshold read at startup and logged with the ExEx head; see `src/main.rs`.
