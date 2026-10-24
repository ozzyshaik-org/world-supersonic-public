#!/usr/bin/env python3
"""
Supersonic World Chain MEV Ingestion Pipeline
Detects arbitrage, priority bribes, bot spam and sandwich candidates and stores them in ClickHouse.

End-to-end fix 2026-10-10 (drafted by omp-1, completed and reviewed by claude-code-1):
- Only fully ingested blocks are analysed: the block row exists, the stored transactions match its
  transactions_count, and it has at least one trace. tank-engine writes blocks, transactions, logs and
  traces in that order (streamer.rs, raw_healer.rs), so a trace means the block is complete. The old
  target, max(world_blocks) - 1, analysed blocks the healer had not refilled yet after the 2026-10-10
  outage, and they got no rows.
- A hole is waited for while tank-engine heals it (an unresolved world_sequence_gaps or world_trace_gaps
  row, at most MEV_HOLE_HEALING_MAX_SEC) and for MEV_HOLE_MAX_WAIT_SEC otherwise (the healer caps a gap
  at 10,000 blocks and never refills the rest). After that it is skipped with a CRITICAL line that names
  the --backfill command. Never silently. A skip stops before any later block a gap row covers, and the rest
  of a skipped run is skipped at once (a run that started as "being healed" stays skipped while it is).
- A block whose analysis keeps failing is skipped loudly after MEV_BLOCK_FAIL_MAX_SEC of continuous failure at
  that position. The first and the latest failure are kept in ingest_checkpoints ('mev_failing',
  'mev_failing_last') because the watchdog restarts a detector that makes no progress every 600 s; a restarted
  process that finds a recent failure at its position starts at one block (a block that times out would
  otherwise cost the whole 600 s just to shrink to), and a gap of more than FAIL_GAP_SEC restarts the clock.
- A chunk is all-or-nothing: a failed query, insert or checkpoint write leaves the checkpoint where it was
  and the chunk is retried. Each failure halves the chunk (the sandwich self-join can pass the 2 GB
  memory cap on 500 busy blocks); successes grow it back to 1000.
- Re-running a range inserts only rows not stored yet (block, type, tx hash, victim, sorted pools; for
  non-sandwich types block, type and tx hash), the sandwich query is DISTINCT (several backruns of one
  victim repeated the row), and alerts fire only for rows actually inserted, after the insert.
- The checkpoint never moves backwards, and a failed checkpoint read is retried instead of falling back
  to a later bootstrap block.
- No ClickHouse connection at import. CLI: --dry-run START END prints what would be inserted
  (read-only client); --backfill START END inserts missing rows without alerts or checkpoint.
"""

import argparse
import json
import logging
import os
import sys
import time

# Config
CH_HOST = os.environ.get("CH_HOST", "127.0.0.1")
CH_PORT = int(os.environ.get("CH_PORT", "8123"))
CH_DB   = os.environ.get("CH_DB", "worldchain")
CHUNK_MAX = 1000
MEV_HOLE_MAX_WAIT_SEC = int(os.environ.get("MEV_HOLE_MAX_WAIT_SEC", "3600"))
MEV_HOLE_HEALING_MAX_SEC = int(os.environ.get("MEV_HOLE_HEALING_MAX_SEC", "43200"))
MEV_BLOCK_FAIL_MAX_SEC = int(os.environ.get("MEV_BLOCK_FAIL_MAX_SEC", "3600"))
FAIL_GAP_SEC = 900  # failures further apart (an outage, a reboot) are not one continuous failure: clock restarts
HOLE_LOG_AFTER_SEC = 30    # the newest batch is briefly incomplete while tank-engine writes its traces
HOLE_LOG_EVERY_SEC = 60
HOLE_ERROR_AFTER_SEC = 600

logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s [MEV-PIPELINE] %(message)s",
    handlers=[logging.StreamHandler(sys.stdout)]
)
log = logging.getLogger("mev")

ch = None  # set by main()

# Hole state: the first incomplete block after the checkpoint, when it was first seen, last log time.
_hole = None
# End of the last skipped run. A hole that starts right after it belongs to the same outage and is skipped at
# once instead of waiting again for every 1000-block window, unless tank-engine is healing it and the run was
# skipped without such a wait (then the healing limit applies afresh).
_skip_run_end = None
_skip_after_healing = False


def get_client(readonly: int = 0):
    import clickhouse_connect
    settings = {'max_memory_usage': 2000000000,
                'max_execution_time': 60,
                'max_threads': 2}
    if readonly:
        settings['readonly'] = readonly
    return clickhouse_connect.get_client(host=CH_HOST, port=CH_PORT, database=CH_DB, settings=settings)


def to_fixed_string(v, size: int) -> str:
    if v is None:
        return '\x00' * size
    if isinstance(v, bytes):
        v = v.decode('utf-8', errors='replace')
    v = str(v).strip()
    v = v.split('\x00')[0]
    if len(v) < size:
        v = v + '\x00' * (size - len(v))
    elif len(v) > size:
        v = v[:size]
    return v


def pool_key(pools) -> tuple:
    """Sorted, padded pool list, first 10: groupArray(DISTINCT) returns pools in no fixed order."""
    return tuple(sorted(to_fixed_string(p, 42) for p in (pools or []) if p)[:10])


def get_last_processed_block() -> int:
    # 1. The checkpoint. A read error is retried until the read succeeds: falling through to a bootstrap
    #    block on a transient error would skip everything between the checkpoint and that block.
    attempt = 0
    while True:
        try:
            res = ch.query("SELECT last_committed_block FROM worldchain.ingest_checkpoints FINAL WHERE name = 'mev'")
            break
        except Exception as e:
            attempt += 1
            log.warning(f"checkpoint read failed (attempt {attempt}): {e}")
            time.sleep(min(60, 5 * attempt))
    if res.result_rows:
        return int(res.result_rows[0][0])
    # 2. No checkpoint row (first start): max(block) from activity, only if not far behind head
    try:
        head = int(ch.query(
            "SELECT block_number FROM worldchain.world_blocks WHERE chain_id = 480 "
            "ORDER BY block_number DESC LIMIT 1").result_rows[0][0])
    except Exception as e:
        log.warning(f"head lookup failed: {e}")
        head = 0
    try:
        res = ch.query("SELECT max(block_number) FROM worldchain.world_mev_activity WHERE chain_id = 480")
        val = res.result_rows[0][0]
        if val and head and (head - int(val)) < 100000:
            return int(val)
        if val and not head:
            return int(val)
        log.warning(f"activity max(block)={val} rejected: head={head} "
                    f"(gap {head - int(val or 0)} >= 100000 — grind rewind guard)")
    except Exception as e:
        log.warning(f"activity max query failed: {e}")
    # 3. Final fallback: 1000 behind head (fresh tip, never a stale constant)
    if head:
        return max(0, head - 1000)
    return 29000000


def update_checkpoint(block: int) -> bool:
    """Write the checkpoint; True on success. greatest() keeps a stale write from moving it backwards."""
    sql = (
        "INSERT INTO worldchain.ingest_checkpoints (name, last_committed_block, updated_at) "
        f"SELECT 'mev', greatest(b, toUInt64({int(block)})), now64(3) "
        "FROM (SELECT ifNull((SELECT max(last_committed_block) FROM worldchain.ingest_checkpoints FINAL "
        "WHERE name = 'mev'), toUInt64(0)) AS b)"
    )
    for attempt in range(3):
        try:
            ch.command(sql)
            return True
        except Exception as e:
            log.error(f"checkpoint write failed (attempt {attempt+1}/3): {e}")
            time.sleep(3 * (attempt + 1))
    log.critical(f"CHECKPOINT WRITE LOST for block {block} after 3 attempts; the chunk will be retried")
    return False


def _store_failing(name: str, block: int, t: float):
    ch.command("INSERT INTO worldchain.ingest_checkpoints (name, last_committed_block, updated_at) "
               f"VALUES ('{name}', {int(block)}, fromUnixTimestamp64Milli(toInt64({int(t * 1000)})))")


def _failing_rows() -> dict:
    res = ch.query("SELECT name, last_committed_block, toUnixTimestamp64Milli(updated_at) "
                   "FROM worldchain.ingest_checkpoints FINAL WHERE name IN ('mev_failing', 'mev_failing_last')")
    return {r[0]: (int(r[1]), int(r[2]) / 1000.0) for r in res.result_rows}


def failing_since(block: int, now: float):
    """Start of the continuous failure at position `block` (the first block after the checkpoint), kept across
    restarts in ingest_checkpoints: 'mev_failing' holds the first failure, 'mev_failing_last' the latest. A new
    position, or a gap longer than FAIL_GAP_SEC since the latest failure, starts the clock again. Records this
    failure. None when ClickHouse cannot be used."""
    try:
        rows = _failing_rows()
        first, latest = rows.get("mev_failing"), rows.get("mev_failing_last")
        if first and latest and first[0] == block and latest[0] == block and now - latest[1] <= FAIL_GAP_SEC:
            since = first[1]
        else:
            since = now
            _store_failing("mev_failing", block, now)
        _store_failing("mev_failing_last", block, now)
        return since
    except Exception as e:
        log.warning(f"failing-block clock not persisted ({e}); using this process's clock")
        return None


def initial_state(now: float) -> dict:
    """Loop state for a new process: resume one block at a time when the previous process was failing here."""
    st = {"last": get_last_processed_block(), "chunk": CHUNK_MAX, "wins": 0, "fails": 0, "fail_block": None}
    try:
        latest = _failing_rows().get("mev_failing_last")
        if latest and latest[0] == st["last"] + 1 and now - latest[1] <= FAIL_GAP_SEC:
            st["chunk"] = 1
            log.warning(f"block {st['last'] + 1} was failing {now - latest[1]:.0f}s ago; resuming one block at a time")
    except Exception as e:
        log.warning(f"failing-block state unreadable ({e}); starting with {CHUNK_MAX}-block chunks")
    return st


def get_target_head() -> int:
    try:
        res = ch.query("SELECT block_number FROM worldchain.world_blocks WHERE chain_id = 480 ORDER BY block_number DESC LIMIT 1")
        if res.result_rows and res.result_rows[0][0]:
            return int(res.result_rows[0][0])
    except Exception as e:
        log.error(f"head lookup failed: {e}")
    return 0


def ingested_blocks(lo: int, hi: int) -> dict:
    """{block: (declared_txs, stored_txs, traced)} for the blocks lo..hi that have a block row. Raises on error."""
    res = ch.query(
        """
        SELECT b.block_number, b.declared, ifNull(t.stored, 0), ifNull(r.traced, 0)
        FROM (SELECT block_number, max(transactions_count) AS declared
              FROM worldchain.world_blocks
              WHERE chain_id = 480 AND block_number >= {lo:UInt64} AND block_number <= {hi:UInt64}
              GROUP BY block_number) AS b
        LEFT JOIN (SELECT block_number, uniqExact(tx_index) AS stored
                   FROM worldchain.world_transactions
                   WHERE chain_id = 480 AND block_number >= {lo:UInt64} AND block_number <= {hi:UInt64}
                   GROUP BY block_number) AS t ON t.block_number = b.block_number
        LEFT JOIN (SELECT block_number, toUInt8(1) AS traced
                   FROM worldchain.world_traces
                   WHERE chain_id = 480 AND block_number >= {lo:UInt64} AND block_number <= {hi:UInt64}
                   GROUP BY block_number) AS r ON r.block_number = b.block_number
        SETTINGS join_use_nulls = 1
        """,
        parameters={"lo": lo, "hi": hi})
    return {int(r[0]): (int(r[1]), int(r[2]), int(r[3])) for r in res.result_rows}


def complete(info) -> bool:
    return info is not None and info[1] >= info[0] and info[2] == 1


def healing(first: int, last_block: int):
    """(is first covered by an unresolved tank-engine gap row, first later block up to last_block that is).
    A failed check counts as covered: keep waiting rather than skip."""
    try:
        res = ch.query(
            """
            SELECT (SELECT count() FROM worldchain.world_sequence_gaps FINAL
                    WHERE resolved = 0 AND gap_lo <= {b:UInt64} AND gap_hi >= {b:UInt64})
                 + (SELECT count() FROM worldchain.world_trace_gaps FINAL
                    WHERE resolved = 0 AND block_number = {b:UInt64}),
                   (SELECT minOrNull(gap_lo) FROM worldchain.world_sequence_gaps FINAL
                    WHERE resolved = 0 AND gap_lo > {b:UInt64} AND gap_lo <= {e:UInt64}),
                   (SELECT minOrNull(block_number) FROM worldchain.world_trace_gaps FINAL
                    WHERE resolved = 0 AND block_number > {b:UInt64} AND block_number <= {e:UInt64})
            """,
            parameters={"b": first, "e": last_block})
        cover, seq_next, trace_next = res.result_rows[0]
        later = [int(x) for x in (seq_next, trace_next) if x is not None]
        return int(cover) > 0, (min(later) if later else None)
    except Exception as e:
        log.warning(f"gap table check failed ({e}); treating block {first} as being healed")
        return True, None


def plan_next(last: int, head: int, now: float):
    """("process", end) | ("skip", end) | ("wait", last). Raises when the ingest check fails."""
    global _hole, _skip_run_end
    end = min(last + CHUNK_MAX, head - 1)
    if end <= last:
        return "wait", last
    info = ingested_blocks(last + 1, end)
    b = last + 1
    while b <= end and complete(info.get(b)):
        b += 1
    if b > last + 1:
        _hole = None
        _skip_run_end = None
        return "process", b - 1
    first = last + 1
    run_end = first
    while run_end + 1 <= end and not complete(info.get(run_end + 1)):
        run_end += 1
    if _hole is None or _hole["start"] != first:
        _hole = {"start": first, "since": now, "logged": None}
    healed, later = healing(first, run_end)
    if later is not None:
        run_end = min(run_end, later - 1)  # a skip never swallows blocks a gap row says are being healed
    age = now - _hole["since"]
    limit = MEV_HOLE_HEALING_MAX_SEC if healed else MEV_HOLE_MAX_WAIT_SEC
    state = "being healed" if healed else "not being healed"
    inherit = (_skip_run_end is not None and first == _skip_run_end + 1 and (not healed or _skip_after_healing))
    if inherit or age >= limit:
        _hole["skip_after_healing"] = _skip_after_healing if inherit else healed
        return "skip", run_end
    if age >= HOLE_LOG_AFTER_SEC and (_hole["logged"] is None or now - _hole["logged"] >= HOLE_LOG_EVERY_SEC):
        _hole["logged"] = now
        bi = info.get(first)
        what = ("no block row" if bi is None else
                f"transactions {bi[1]}/{bi[0]}" + ("" if bi[2] else ", no traces"))
        msg = (f"waiting for block {first} to be fully ingested ({what}; {state}) for {age:.0f}s; "
               f"skipped after {limit}s")
        (log.error if age >= HOLE_ERROR_AFTER_SEC else log.warning)(msg)
    return "wait", last


def _existing_keys(start: int, end: int):
    """(full_keys, coarse_keys) of rows already stored for the range. Raises on error."""
    res = ch.query(
        """
        SELECT block_number, mev_type, tx_hash, sandwich_victim_tx, target_pools
        FROM worldchain.world_mev_activity
        WHERE chain_id = 480 AND block_number >= {start:UInt64} AND block_number <= {end:UInt64}
        """,
        parameters={"start": start, "end": end})
    full_keys, coarse_keys = set(), set()
    for r in res.result_rows:
        victim = to_fixed_string(r[3], 66) if r[3] is not None else None
        k = (int(r[0]), str(r[1]), to_fixed_string(r[2], 66), victim, pool_key(r[4]))
        full_keys.add(k)
        if k[1] != "sandwich_candidate":
            coarse_keys.add(k[:3])
    return full_keys, coarse_keys


def row_key(r: dict) -> tuple:
    victim = r.get("sandwich_victim_tx")
    return (int(r["block_number"]), r["mev_type"], to_fixed_string(r["tx_hash"], 66),
            to_fixed_string(victim, 66) if victim is not None else None, pool_key(r.get("target_pools")))


def detect(start: int, end: int) -> list:
    """Candidate rows for start..end from the four detectors. Any query error raises."""
    inserts = []

    # 1. PRIORITY BRIBES (max_priority_fee_wei >= 1e8 wei)
    res = ch.query(
        """
        SELECT block_number, tx_hash, tx_index, max_priority_fee_wei
        FROM worldchain.world_transactions
        WHERE chain_id = 480 AND block_number >= {start:UInt64} AND block_number <= {end:UInt64}
          AND max_priority_fee_wei >= 100000000
        """,
        parameters={"start": start, "end": end}
    )
    for row in res.result_rows:
        inserts.append({
            "block_number": int(row[0]),
            "tx_hash": to_fixed_string(row[1], 66),
            "tx_index": int(row[2]),
            "mev_type": "priority_bribe",
            "extracted_value_wei": int(row[3]),
            "target_pools": [],
            "flash_loan_provider": None,
            "flash_loan_asset": None,
            "flash_loan_amount": None,
            "sandwich_victim_tx": None
        })

    # 2. BOT SPAM (Same address, >= 5 transactions in same block)
    res = ch.query(
        """
        SELECT block_number, from_address, count() as tx_count, groupArray(tx_hash) as hashes, groupArray(tx_index) as indices
        FROM worldchain.world_transactions
        WHERE chain_id = 480 AND block_number >= {start:UInt64} AND block_number <= {end:UInt64}
          AND from_address != '0xdeaddeaddeaddeaddeaddeaddeaddeaddead0001'
        GROUP BY block_number, from_address
        HAVING tx_count >= 5
        """,
        parameters={"start": start, "end": end}
    )
    for row in res.result_rows:
        hashes = row[3]
        indices = row[4]
        if hashes and indices:
            inserts.append({
                "block_number": int(row[0]),
                "tx_hash": to_fixed_string(hashes[0], 66),
                "tx_index": int(indices[0]),
                "mev_type": "bot_spam",
                "extracted_value_wei": 0,
                "target_pools": [],
                "flash_loan_provider": None,
                "flash_loan_asset": None,
                "flash_loan_amount": None,
                "sandwich_victim_tx": None
            })

    # 3. ATOMIC ARBITRAGE (ZCode rewrite 2026-09-09 — DRAFT 3, Bug B fix)
    # Net ETH flow of the tx SENDER across internal transfers, profitable-only (net after gas > 0),
    # real tx_index, plausibility bound.
    res = ch.query(
        """
        SELECT
            t.block_number,
            tx.tx_index,
            tx.tx_hash,
            tx.from_address AS sender,
            sum(multiIf(t.to_address = tx.from_address, toInt256(t.value_wei), t.from_address = tx.from_address, -toInt256(t.value_wei), toInt256(0))) AS net_in_to_sender,
            any(tx.gas_used * tx.effective_gas_price_wei) AS gas_cost_wei,
            groupArray(DISTINCT t.to_address) AS pools
        FROM worldchain.world_traces AS t
        INNER JOIN worldchain.world_transactions AS tx
            ON t.chain_id = 480 AND t.block_number = tx.block_number
           AND t.tx_index = tx.tx_index
        WHERE t.block_number >= {start:UInt64} AND t.block_number <= {end:UInt64}
          AND t.is_canonical = 1
        GROUP BY t.block_number, tx.tx_index, tx.tx_hash, tx.from_address
        HAVING net_in_to_sender > 1000000000000
        """,
        parameters={"start": start, "end": end}
    )
    for row in res.result_rows:
        blk = int(row[0])
        tx_index = int(row[1])
        tx_hash = to_fixed_string(row[2], 66)
        net_flow = int(row[4])
        gas_cost = int(row[5]) if row[5] is not None else 0
        net_extracted = net_flow - gas_cost
        if net_extracted <= 0:
            continue
        if net_extracted > 1000 * 10**18:
            log.warning(
                f"implausible net_extracted {net_extracted} wei for {tx_hash}; skipped")
            continue
        inserts.append({
            "block_number": blk,
            "tx_hash": tx_hash,
            "tx_index": tx_index,
            "mev_type": "atomic_arbitrage",
            "extracted_value_wei": net_extracted,
            "_gas_cost_wei": gas_cost,
            "target_pools": list(pool_key(row[6])),
            "flash_loan_provider": None,
            "flash_loan_asset": None,
            "flash_loan_amount": None,
            "sandwich_victim_tx": None
        })

    # 4. SANDWICH ATTACK DETECTION (Primary-Key Aligned Non-AA). DISTINCT: s3 (the backrun) is joined but
    # not selected, so a victim with two backruns by the attacker produced the same row twice.
    res = ch.query(
        """
        WITH pool_swaps AS (
            SELECT
                l.block_number,
                l.tx_index,
                l.tx_hash,
                l.address AS pool_address,
                tx.from_address AS sender_eoa,
                tx.to_address AS target_contract
            FROM worldchain.world_logs AS l
            INNER JOIN worldchain.world_transactions AS tx
                ON tx.chain_id = 480 AND l.chain_id = 480 AND l.block_number = tx.block_number AND l.tx_index = tx.tx_index
            WHERE l.chain_id = 480 AND l.block_number >= {start:UInt64} AND l.block_number <= {end:UInt64}
              AND length(l.topics) >= 1
              AND lower(l.topics[1]) IN (
                '0xc42079f94a6350d7e6235f29174924f928cc2ac818eb64fed8004e115fbcca67',
                '0xd78ad95fa46c994b6551d0da85fc275fe613ce37657fb8d5e3d130840159d822'
              )
              AND tx.to_address != '0x0000000071727de22e5e9d8baf0edac6f37da032'
        ),
        sandwiches AS (
            SELECT
                s1.block_number AS block_number,
                s1.pool_address AS pool_address,
                s1.sender_eoa AS attacker_address,
                s1.tx_hash AS frontrun_tx,
                s1.tx_index AS frontrun_index,
                s2.tx_hash AS victim_tx
            FROM pool_swaps AS s1
            INNER JOIN pool_swaps AS s2
                ON s1.block_number = s2.block_number
               AND s1.pool_address = s2.pool_address
               AND s1.tx_index < s2.tx_index
               AND s1.sender_eoa != s2.sender_eoa
            INNER JOIN pool_swaps AS s3
                ON s2.block_number = s3.block_number
               AND s2.pool_address = s3.pool_address
               AND s2.tx_index < s3.tx_index
               AND s1.sender_eoa = s3.sender_eoa
        )
        SELECT DISTINCT
            block_number,
            frontrun_tx,
            frontrun_index,
            pool_address,
            victim_tx
        FROM sandwiches
        """,
        parameters={"start": start, "end": end}
    )
    for row in res.result_rows:
        inserts.append({
            "block_number": int(row[0]),
            "tx_hash": to_fixed_string(row[1], 66),
            "tx_index": int(row[2]),
            "mev_type": "sandwich_candidate",
            "extracted_value_wei": 0,
            "target_pools": [to_fixed_string(row[3], 42)],
            "flash_loan_provider": None,
            "flash_loan_asset": None,
            "flash_loan_amount": None,
            "sandwich_victim_tx": to_fixed_string(row[4], 66)
        })
    return inserts


def new_rows_only(inserts: list, start: int, end: int) -> list:
    """Drop rows already stored for the range and repeats within the batch. Raises on a read error."""
    stored_full, stored_coarse = _existing_keys(start, end)
    seen, out = set(), []
    for r in inserts:
        k = row_key(r)
        if k in stored_full or k in seen:
            continue
        # One atomic/bribe/bot row per (block, type, tx): its pool slice may differ between runs.
        if r["mev_type"] != "sandwich_candidate" and k[:3] in stored_coarse:
            continue
        seen.add(k)
        out.append(r)
    return out


def alert(rows: list):
    """High-value alerts (atomic_arbitrage >= 1e12 wei) for rows that were just inserted."""
    for r in rows:
        if r.get('mev_type') == 'atomic_arbitrage' and r.get('extracted_value_wei', 0) >= 1000000000000:
            try:
                from alert_hook import trigger_alert
                from mev_enrich import enrich
                _enr_text, _ = enrich(str(r['tx_hash']).strip(),
                    r.get('block_number', 0),
                    r['extracted_value_wei'],
                    r.get('_gas_cost_wei', 0),
                    r.get('target_pools', []))
                trigger_alert(r['tx_hash'], r['extracted_value_wei'],
                              r['mev_type'], enriched_text=_enr_text)
            except Exception as e:
                log.error(f'Failed to execute alert trigger: {e}')


def process_block_range(start: int, end: int, mode: str = "live") -> bool:
    """Detect MEV in start..end and store the new rows. mode: live (alerts), backfill (no alerts), dry-run
    (print, no writes). True on success; False on any query or insert error (nothing stored, so the
    caller retries the same range)."""
    log.info(f"Processing MEV detection for block range {start} -> {end}")
    try:
        inserts = detect(start, end)
        if mode == "dry-run":
            for r in inserts:
                print(json.dumps({k: v for k, v in r.items() if not k.startswith("_")}))
            return True
        rows = new_rows_only(inserts, start, end) if inserts else []
        if rows:
            ch.insert(
                "worldchain.world_mev_activity",
                [[
                    r["block_number"],
                    r["tx_hash"],
                    r["tx_index"],
                    r["mev_type"],
                    r["extracted_value_wei"],
                    r["target_pools"],
                    r["flash_loan_provider"],
                    r["flash_loan_asset"],
                    r["flash_loan_amount"],
                    r["sandwich_victim_tx"]
                ] for r in rows],
                column_names=[
                    "block_number", "tx_hash", "tx_index", "mev_type",
                    "extracted_value_wei", "target_pools", "flash_loan_provider",
                    "flash_loan_asset", "flash_loan_amount", "sandwich_victim_tx"
                ]
            )
            log.info(f"Successfully inserted {len(rows)} MEV activity rows "
                     f"({len(inserts) - len(rows)} already stored).")
        elif inserts:
            log.info(f"All {len(inserts)} MEV rows for {start} -> {end} are already stored.")
    except Exception as e:
        log.error(f"MEV detection for {start} -> {end} failed, nothing stored: {e}")
        return False
    if mode == "live":
        alert(rows)
    return True


def run_range(start: int, end: int, mode: str) -> bool:
    """--dry-run / --backfill: walk start..end in chunks that halve on failure. False if a single block fails."""
    cur, chunk = start, CHUNK_MAX
    if mode == "backfill":
        for lo in range(start, end + 1, CHUNK_MAX):
            hi = min(end, lo + CHUNK_MAX - 1)
            info = ingested_blocks(lo, hi)
            bad = [b for b in range(lo, hi + 1) if not complete(info.get(b))]
            if bad:
                log.warning(f"{len(bad)} block(s) in {lo} -> {hi} are not fully ingested (first {bad[:5]}); "
                            f"their rows may be incomplete")
    while cur <= end:
        stop = min(end, cur + chunk - 1)
        if process_block_range(cur, stop, mode):
            cur = stop + 1
            chunk = min(CHUNK_MAX, chunk * 2)
        elif chunk > 1:
            chunk = max(1, chunk // 2)
        else:
            log.error(f"block {cur} fails even alone; stopping at {cur}")
            return False
    return True


def step(st: dict, now: float) -> float:
    """One loop iteration. st: last, chunk, wins, fails. Returns the seconds to sleep."""
    global _skip_run_end, _skip_after_healing
    head = get_target_head()
    action, upto = plan_next(st["last"], head, now)
    if action == "process":
        end = min(upto, st["last"] + st["chunk"])
        if process_block_range(st["last"] + 1, end) and update_checkpoint(end):
            st["last"], st["fails"], st["fail_block"] = end, 0, None
            st["wins"] += 1
            if st["chunk"] < CHUNK_MAX and st["wins"] >= 3:
                st["chunk"], st["wins"] = min(CHUNK_MAX, st["chunk"] * 2), 0
            return 0
        st["fails"], st["wins"] = st["fails"] + 1, 0
        pos = st["last"] + 1  # the failure clock is kept per position, from the first failed chunk there
        since = failing_since(pos, now)
        if since is not None:
            st["fail_since"] = since
        elif st.get("fail_block") != pos or now - st.get("fail_last", now) > FAIL_GAP_SEC:
            st["fail_since"] = now
        st["fail_block"], st["fail_last"] = pos, now
        if end == pos:  # one block that fails on its own: bounded, then skipped loudly
            if now - st["fail_since"] >= MEV_BLOCK_FAIL_MAX_SEC:
                log.critical(f"SKIPPING block {end}: its analysis has failed for {now - st['fail_since']:.0f}s; "
                             f"retry it with: mev_detector.py --backfill {end} {end}")
                if update_checkpoint(end):
                    st["last"], st["fails"], st["fail_block"] = end, 0, None
                    return 0
        st["chunk"] = max(1, st["chunk"] // 2)
        # Shrinking is about size, so it is quick; only a single block backs off (5 s doubling to 60 s).
        pause = 2 if end > st["last"] + 1 else min(60, 5 * 2 ** min(st["fails"] - 1, 4))
        log.warning(f"chunk {st['last'] + 1} -> {end} not committed; retrying with up to {st['chunk']} "
                    f"block(s) in {pause}s (failure {st['fails']})")
        return pause
    if action == "skip":
        log.critical(f"SKIPPING blocks {st['last'] + 1} -> {upto}: not fully ingested after the wait limit; "
                     f"analyse them once they exist with: mev_detector.py --backfill {st['last'] + 1} {upto}")
        if update_checkpoint(upto):
            st["last"], _skip_run_end = upto, upto
            _skip_after_healing = bool(_hole and _hole.get("skip_after_healing"))
            return 0
        return 10
    return 0.5


def main():
    global ch
    p = argparse.ArgumentParser(description="Supersonic World Chain MEV detector")
    g = p.add_mutually_exclusive_group()
    g.add_argument("--dry-run", nargs=2, type=int, metavar=("START", "END"),
                   help="read-only: print the rows START..END would get; no insert, no checkpoint, no alerts")
    g.add_argument("--backfill", nargs=2, type=int, metavar=("START", "END"),
                   help="insert rows missing for START..END; no alerts, checkpoint untouched")
    args = p.parse_args()

    if args.dry_run or args.backfill:
        mode = "dry-run" if args.dry_run else "backfill"
        start, end = args.dry_run or args.backfill
        ch = get_client(readonly=2 if mode == "dry-run" else 0)
        sys.exit(0 if run_range(start, end, mode) else 1)

    ch = get_client()
    log.info("ClickHouse connection established successfully.")
    log.info("Starting MEV detection daemon...")
    st = initial_state(time.time())
    log.info(f"Initial MEV checkpoint resolved: block={st['last']}")
    while True:
        try:
            pause = step(st, time.time())
        except Exception as e:
            log.error(f"Error in execution loop: {e}")
            pause = 10
        if pause:
            time.sleep(pause)


if __name__ == "__main__":
    main()
