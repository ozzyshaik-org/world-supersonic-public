# Schema

## Quick start (one path)

```bash
clickhouse-client --multiquery < schema/quickstart_schema.sql
```

Run it against an empty ClickHouse. It creates the `worldchain` database and the 24 tables listed below,
and nothing else (no data, no materialized views, no dictionaries). Every statement is `CREATE ... IF NOT EXISTS`,
so re-running it is safe: existing tables and their rows are left untouched. It does **not** migrate an existing table to
a changed definition.

## Where the DDL comes from

The column, engine, `ORDER BY`, `PARTITION BY` and projection definitions are copied unchanged from the table DDL in
`complete_schema.sql` (a raw `SHOW CREATE TABLE` dump). Mechanical changes only: `IF NOT EXISTS`, dump escaping decoded,
and deployment-specific settings removed (`storage_policy`, every `TTL` clause, deduplication-window and other
merge/part tuning). No table was written by hand. Because every `TTL` is removed, nothing expires automatically;
add retention yourself if you need it.

## Required tables

Derived by searching the Go and Python sources for `worldchain.<table>` references (and a second search for bare
table identifiers) in the four services. Line numbers are in the tree at the time of writing and will drift.
Every table below exists in `complete_schema.sql`, so there is no table the services need that this repository's
public sources cannot define.

| Table | Referenced in |
|---|---|
| `world_blocks` | `cmd/rpc-gateway/main.go`:650,1480<br>`cmd/api/contracts.go`:1428<br>`sgip/main.py`:602<br>`mev-detector/mev_detector.py`:10,118,205,219 |
| `world_transactions` | `cmd/rpc-gateway/main.go`:591<br>`sgip/main.py`:220,485<br>`mev-detector/mev_detector.py`:223,338,362,401,451 |
| `world_logs` | `mev-detector/mev_detector.py`:450<br>`mev-detector/mev_enrich.py`:31 |
| `world_traces` | `sgip/main.py`:277,598<br>`mev-detector/mev_detector.py`:227,400 |
| `world_token_transfers` | `cmd/rpc-gateway/main.go`:781,784,787 |
| `world_address_labels` | `sgip/main.py`:97,156 |
| `world_abi_signatures` | `sgip/main.py`:111 |
| `world_contracts` | `cmd/api/contracts.go`:187,286,364,417,1079,1283<br>`sgip/main.py`:145,552,599,600 |
| `world_contract_interactions` | `cmd/api/contracts.go`:543,1179,1187<br>`sgip/main.py`:181,373,386,601 |
| `world_contract_functions` | `cmd/api/contracts.go`:474 |
| `world_mev_activity` | `cmd/rpc-gateway/main.go`:1087,1090,1093,1096<br>`cmd/api/contracts.go`:622<br>`mev-detector/mev_detector.py`:124,310,555 |
| `world_mev_hourly_stats` | `sgip/main.py`:627 |
| `world_trace_gaps` | `mev-detector/mev_detector.py`:12,248,252 |
| `world_sequence_gaps` | `mev-detector/mev_detector.py`:12,246,250 |
| `ingest_checkpoints` | `mev-detector/mev_detector.py`:18,107,143,145,160,166,172 |
| `latest_oracle_prices` | `mev-detector/mev_enrich.py`:24 |
| `api_keys` | `cmd/rpc-gateway/main.go`:1300,1340<br>`cmd/api/main.go`:61 |
| `api_depeg_drift` | `cmd/api/contracts.go`:1348 |
| `api_dex_swaps` | `cmd/rpc-gateway/main.go`:891,894,897<br>`cmd/api/main.go`:101 |
| `api_gas_by_function` | `cmd/api/contracts.go`:994 |
| `api_morpho_activity` | `cmd/api/main.go`:111 |
| `api_nft_transfers` | `cmd/api/main.go`:121 |
| `api_token_balances` | `cmd/rpc-gateway/main.go`:701,1187<br>`cmd/api/contracts.go`:682 |
| `api_universal_defi_events` | `cmd/rpc-gateway/main.go`:995,998,1001 |

## What this does and does not give you

* The `world_*` tables, `api_keys`, `ingest_checkpoints` and the other service tables exist and are empty. The
  `api_*` tables (`api_dex_swaps`, `api_token_balances`, ...) are filled in production by materialized views that are
  not published here, so they stay empty unless you populate them. Endpoints backed by them return empty results.
* Without data, some endpoints return errors rather than empty results (for example `/v1/block/latest` on `rpc-gateway`
  and `/api/block/{n}` on `sgip` returned HTTP 500 on an empty database in the test run). Those are application
  behaviours on empty data, not missing tables.
* `01_core_schema.sql` and `02_materialized_views.sql` are an older core-only schema. They create a different
  `world_logs` (`topic0`..`topic3`, no `topics` array or `block_timestamp`) than the one the services query, they
  do not create the `worldchain` database, and they lack most of the tables above. Use `quickstart_schema.sql` instead.
  `complete_schema.sql`, `live_schema_2026.sql`, `deploy_v2_tables.sql` and `world_transactions_v2.sql` are not quick-start
  files (`complete_schema.sql` does not apply as-is; `live_schema_2026.sql` contains `DROP TABLE`).
