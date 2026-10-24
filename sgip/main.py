#!/usr/bin/env python3
"""
SGIP — Supersonic Graph Intelligence Platform (v1.1 Hardened)
Interactive blockchain visualization service for World Chain L2.

Port: 8095
Standalone dashboard + embeddable widget mode.
"""

import gc
import json
import re
import subprocess
import threading
import os
import time
import functools
from typing import Optional

import psutil
import clickhouse_connect
from fastapi import FastAPI, HTTPException, Query
from fastapi.responses import HTMLResponse, JSONResponse
from fastapi.middleware.cors import CORSMiddleware

# ── Config ────────────────────────────────────────────────────────────────────
CH_HOST = os.environ.get("CH_HOST", "127.0.0.1")
CH_PORT = int(os.environ.get("CH_PORT", "8123"))
CH_DB   = os.environ.get("CH_DB", "worldchain")
VERSION = "1.1.0"
MAX_ALLOWED_RSS_BYTES = 220 * 1024 * 1024  # 220MB limit for FastAPI API worker process

# ── ClickHouse pool ───────────────────────────────────────────────────────────
_thread_local = threading.local()

def ch():
    if not hasattr(_thread_local, "client") or _thread_local.client is None:
        _thread_local.client = clickhouse_connect.get_client(
            host=CH_HOST, port=CH_PORT, database=CH_DB,
            connect_timeout=10, send_receive_timeout=30
        )
    return _thread_local.client

# ── App ───────────────────────────────────────────────────────────────────────
app = FastAPI(title="SGIP", version=VERSION, docs_url="/api/docs")

# ── Memory Guard Middleware ───────────────────────────────────────────────────
@app.middleware("http")
async def memory_guard_middleware(request, call_next):
    process = psutil.Process()
    rss = process.memory_info().rss
    if rss > MAX_ALLOWED_RSS_BYTES:
        gc.collect()
        if any(p in request.url.path for p in ["/api/top", "/api/stats", "/api/tx", "/api/contract", "/api/wallet"]):
            return JSONResponse(
                status_code=503,
                content={"status": "error", "message": "Service shedding load under high memory pressure"}
            )
    return await call_next(request)

app.add_middleware(
    CORSMiddleware,
    allow_origins=["*"],
    allow_methods=["GET"],
    allow_headers=["*"],
)

# ── Caching & Lock Constructs ──────────────────────────────────────────────────
_GRAPH_CACHE = {}
_TOP_CACHE = {}
_TOP_LOCK = threading.Lock()

# ── Helpers ───────────────────────────────────────────────────────────────────
def addr_short(a) -> str:
    if not a: return ''
    if isinstance(a, bytes): a = a.decode('utf-8', errors='replace')
    a = str(a)
    if len(a) < 10: return a
    return a[:6] + "..." + a[-4:]

def normalize(v) -> str:
    """Normalize ClickHouse value to plain Python str."""
    if v is None: return ''
    if isinstance(v, bytes): return v.decode('utf-8', errors='replace')
    return str(v)

def is_tx_hash(s: str) -> bool:
    return bool(re.fullmatch(r'0x[0-9a-fA-F]{64}', s))

def is_address(s: str) -> bool:
    return bool(re.fullmatch(r'0x[0-9a-fA-F]{40}', s))

def get_labels(addresses: list) -> dict:
    if not addresses: return {}
    try:
        res = ch().query(
            "SELECT address, label, label_category FROM worldchain.world_address_labels WHERE address IN ({addrs:Array(String)}) SETTINGS max_execution_time=5, max_threads=2",
            parameters={"addrs": [a.lower() for a in addresses]}
        )
        return {normalize(row[0]).lower(): (normalize(row[1]), normalize(row[2])) for row in res.result_rows}
    except Exception:
        return {}

@functools.lru_cache(maxsize=1024)
def decode_selector(selector: str) -> str:
    if not selector: return ""
    selector = selector[:10].lower()
    padded = selector.ljust(32, chr(0))
    try:
        res = ch().query(
            "SELECT text_signature FROM worldchain.world_abi_signatures WHERE selector_or_topic = {s:String} LIMIT 1 SETTINGS max_execution_time=5, max_threads=2",
            parameters={"s": padded}
        )
        if res.result_rows:
            return normalize(res.result_rows[0][0])
    except Exception:
        pass
    return selector

# ═════════════════════════════════════════════════════════════════════════════
# API ROUTES
# ═════════════════════════════════════════════════════════════════════════════

@app.get("/health")
def health():
    process = psutil.Process()
    return {
        "status": "ok",
        "service": "sgip",
        "version": VERSION,
        "rss_mb": round(process.memory_info().rss / (1024 * 1024), 2),
        "cpu_percent": process.cpu_percent()
    }


@app.get("/api/search")
def search(q: str = Query(..., min_length=4)):
    """Auto-detect input and return redirect info or search suggestions."""
    q = q.strip().lower()
    if is_tx_hash(q):
        return {"type": "tx", "value": q}
    if is_address(q):
        try:
            res = ch().query(
                "SELECT count() FROM worldchain.world_contracts WHERE address = {a:String} SETTINGS max_execution_time=5, max_threads=2",
                parameters={"a": q}
            )
            if res.result_rows[0][0] > 0:
                return {"type": "contract", "value": q}
        except Exception:
            pass
        return {"type": "wallet", "value": q}

    try:
        res = ch().query(
            "SELECT address, label FROM worldchain.world_address_labels WHERE lower(label) LIKE {q:String} LIMIT 10 SETTINGS max_execution_time=5, max_threads=2",
            parameters={"q": f"%{q}%"}
        )
        if res.result_rows:
            suggestions = [{"address": normalize(r[0]), "label": normalize(r[1])} for r in res.result_rows]
            return {"type": "suggestions", "value": suggestions}
    except Exception:
        pass

    return {"type": "unknown", "value": q}


@app.get("/api/top")
def get_top_contracts(limit: int = 20):
    cache_key = f"top:{limit}"
    if cache_key in _TOP_CACHE:
        return JSONResponse(_TOP_CACHE[cache_key])

    with _TOP_LOCK:
        if cache_key in _TOP_CACHE:
            return JSONResponse(_TOP_CACHE[cache_key])
        try:
            res = ch().query(
                """
                SELECT to_address, sum(call_count) as total_calls, 0 as unique_callers
                FROM worldchain.world_contract_interactions
                WHERE chain_id = 480
                GROUP BY to_address
                ORDER BY total_calls DESC
                LIMIT {lim:Int32}
                SETTINGS max_threads = 2, max_execution_time = 10, max_memory_usage = 6000000000
                """,
                parameters={"lim": limit}
            )
            rows = res.result_rows
            addrs = [normalize(r[0]) for r in rows]
            labels_map = get_labels(addrs)

            results = []
            for r in rows:
                addr = normalize(r[0])
                lbl, cat = labels_map.get(addr.lower(), ("", ""))
                results.append({
                    "address": addr,
                    "label": lbl or addr_short(addr),
                    "category": cat or "contract",
                    "total_calls": int(r[1]),
                    "unique_callers": int(r[2])
                })
            _TOP_CACHE[cache_key] = results
            return JSONResponse(results)
        except Exception as e:
            if cache_key in _TOP_CACHE:
                return JSONResponse(_TOP_CACHE[cache_key])
            raise HTTPException(500, str(e))


@app.get("/api/block/{block_number}")
def get_block_txs(block_number: int):
    """Get all transactions in a block with labeled addresses."""
    try:
        res = ch().query(
            """
            SELECT tx_hash, from_address, to_address, toFloat64(value_wei)/1e18, status, gas_used
            FROM worldchain.world_transactions
            WHERE block_number = {b:Int64}
            LIMIT 500
            SETTINGS max_threads = 2, max_execution_time = 10, max_memory_usage = 6000000000
            """,
            parameters={"b": block_number}
        )
        rows = res.result_rows
        if not rows:
            raise HTTPException(404, f"No transactions found for block {block_number}")

        addrs = set()
        for r in rows:
            addrs.add(normalize(r[1]).lower())
            if r[2]: addrs.add(normalize(r[2]).lower())
        labels_map = get_labels(list(addrs))

        txs = []
        for r in rows:
            tx_hash = normalize(r[0])
            frm = normalize(r[1])
            to = normalize(r[2]) if r[2] else ""
            frm_label = labels_map.get(frm.lower(), ("", ""))[0]
            to_label = labels_map.get(to.lower(), ("", ""))[0] if to else ""
            txs.append({
                "hash": tx_hash,
                "from_address": frm,
                "from_label": frm_label or addr_short(frm),
                "to_address": to,
                "to_label": to_label or (addr_short(to) if to else "Contract Deployment"),
                "value_eth": round(float(r[3]), 6),
                "status": int(r[4]),
                "gas_used": int(r[5])
            })
        return JSONResponse({"block_number": block_number, "transactions": txs})
    except Exception as e:
        raise HTTPException(500, str(e))


@app.get("/api/tx/{tx_hash}")
def get_tx_graph(tx_hash: str):
    """Return Cytoscape.js graph elements for a transaction trace."""
    tx_hash = tx_hash.lower()
    if not is_tx_hash(tx_hash):
        raise HTTPException(400, "Invalid transaction hash")
    cache_key = f"tx:{tx_hash}"
    if cache_key in _GRAPH_CACHE:
        return JSONResponse(_GRAPH_CACHE[cache_key])

    try:
        res = ch().query(
            """
            SELECT
                trace_index, depth,
                from_address, to_address,
                toFloat64(value_wei) / 1e18  as eth_value,
                trace_type, error, input_data
            FROM worldchain.world_traces
            WHERE tx_hash = {h:String}
              AND is_canonical = 1
            ORDER BY trace_index ASC
            LIMIT 500
            SETTINGS max_threads = 2, max_execution_time = 10, max_memory_usage = 6000000000
            """,
            parameters={"h": tx_hash}
        )
    except Exception as e:
        raise HTTPException(500, f"ClickHouse error: {e}")

    rows = res.result_rows
    if not rows:
        raise HTTPException(404, f"No traces found for {tx_hash}")

    nodes = {}
    edges = []

    unique_addrs = set()
    for row in rows:
        unique_addrs.add(normalize(row[2]).lower())
        unique_addrs.add(normalize(row[3]).lower())
    labels_map = get_labels(list(unique_addrs))

    for row in rows:
        idx, depth, frm, to, eth, t_type, err, inp = row
        frm = normalize(frm); to = normalize(to)
        t_type = normalize(t_type); err = normalize(err); inp = normalize(inp)

        for addr in [frm, to]:
            if addr and addr not in nodes:
                lbl, cat = labels_map.get(addr.lower(), ("", ""))
                display_label = f"{lbl} ({addr_short(addr)})" if lbl else addr_short(addr)
                nodes[addr] = {
                    "id": addr,
                    "label": display_label,
                    "full": addr,
                    "depth": depth,
                    "type": "address",
                    "category": cat or "wallet",
                    "has_error": False,
                }
        if frm and to:
            nodes[to]["depth"] = min(nodes[to].get("depth", 999), depth)
            if err:
                nodes[to]["has_error"] = True

            selector = inp[:10] if inp else ""
            decoded = decode_selector(selector) if selector else ""
            edge_label = decoded if decoded else (t_type.upper() if not err else f"{t_type.upper()} REVERT")

            edges.append({
                "id": f"e{idx}",
                "source": frm,
                "target": to,
                "label": edge_label,
                "eth_value": round(float(eth), 6),
                "trace_type": t_type,
                "error": err or "",
                "input_prefix": selector,
                "decoded_signature": decoded
            })

    elements = [{"data": nd} for nd in nodes.values()] + [{"data": e} for e in edges]
    meta = {
        "tx_hash": tx_hash,
        "total_traces": len(rows),
        "unique_addresses": len(nodes),
    }
    res_payload = {"elements": elements, "meta": meta}
    if len(_GRAPH_CACHE) > 2000:
        _GRAPH_CACHE.clear()
    _GRAPH_CACHE[cache_key] = res_payload
    return JSONResponse(res_payload)


@app.get("/api/contract/{address}")
def get_contract_graph(
    address: str,
    limit: int = Query(30, ge=1, le=100)
):
    """Return dependency graph (callers + callees) for a contract."""
    address = address.lower()
    if not is_address(address):
        raise HTTPException(400, "Invalid address")
    cache_key = f"contract:{address}:{limit}"
    if cache_key in _GRAPH_CACHE:
        return JSONResponse(_GRAPH_CACHE[cache_key])

    try:
        in_res = ch().query(
            """
            SELECT from_address, function_selector,
                   sum(call_count) as calls,
                   sum(error_count) as errs
            FROM worldchain.world_contract_interactions
            WHERE chain_id = 480 AND to_address = {a:FixedString(42)} GROUP BY from_address, function_selector
            ORDER BY calls DESC
            LIMIT {lim:Int32}
            SETTINGS max_threads = 2, max_execution_time = 10, max_memory_usage = 6000000000
            """,
            parameters={"a": address, "lim": limit}
        )
        out_res = ch().query(
            """
            SELECT to_address, function_selector,
                   sum(call_count) as calls,
                   sum(error_count) as errs
            FROM worldchain.world_contract_interactions
            WHERE chain_id = 480 AND from_address = {a:FixedString(42)} GROUP BY to_address, function_selector
            ORDER BY calls DESC
            LIMIT {lim:Int32}
            SETTINGS max_threads = 2, max_execution_time = 10, max_memory_usage = 6000000000
            """,
            parameters={"a": address, "lim": limit}
        )

        unique_addrs = {address}
        for r in in_res.result_rows: unique_addrs.add(normalize(r[0]).lower())
        for r in out_res.result_rows: unique_addrs.add(normalize(r[0]).lower())
        labels_map = get_labels(list(unique_addrs))
    except Exception as e:
        raise HTTPException(500, f"ClickHouse error: {e}")

    def node(addr, role):
        addr = normalize(addr)
        lbl, cat = labels_map.get(addr.lower(), ("", ""))
        display_label = f"{lbl} ({addr_short(addr)})" if lbl else addr_short(addr)
        return {
            "id": addr,
            "label": display_label,
            "full": addr,
            "role": role,
            "category": cat or "contract",
            "is_target": addr == address,
        }

    nodes = {address: node(address, "target")}
    edges = []

    for row in in_res.result_rows:
        frm, sel, calls, errs = normalize(row[0]), normalize(row[1]), row[2], row[3]
        if frm not in nodes:
            nodes[frm] = node(frm, "caller")
        err_pct = round((errs / calls) * 100, 1) if calls > 0 else 0
        decoded = decode_selector(sel)
        edges.append({
            "id": f"in_{frm}_{sel}",
            "source": frm,
            "target": address,
            "label": f"{decoded or sel} ({int(calls):,})",
            "calls": int(calls),
            "errors": int(errs),
            "err_pct": err_pct,
            "selector": sel,
            "decoded_signature": decoded,
            "direction": "in",
        })

    for row in out_res.result_rows:
        to, sel, calls, errs = normalize(row[0]), normalize(row[1]), row[2], row[3]
        if to not in nodes:
            nodes[to] = node(to, "callee")
        err_pct = round((errs / calls) * 100, 1) if calls > 0 else 0
        decoded = decode_selector(sel)
        edges.append({
            "id": f"out_{to}_{sel}",
            "source": address,
            "target": to,
            "label": f"{decoded or sel} ({int(calls):,})",
            "calls": int(calls),
            "errors": int(errs),
            "err_pct": err_pct,
            "selector": sel,
            "decoded_signature": decoded,
            "direction": "out",
        })

    elements = [{"data": nd} for nd in nodes.values()] + [{"data": e} for e in edges]
    meta = {
        "address": address,
        "caller_count": len(in_res.result_rows),
        "callee_count": len(out_res.result_rows),
    }
    res_payload = {"elements": elements, "meta": meta}
    if len(_GRAPH_CACHE) > 2000:
        _GRAPH_CACHE.clear()
    _GRAPH_CACHE[cache_key] = res_payload
    return JSONResponse(res_payload)


@app.get("/api/wallet/{address}")
def get_wallet_graph(address: str, limit: int = Query(20, ge=1, le=50)):
    """Return wallet interaction graph."""
    address = address.lower()
    if not is_address(address):
        raise HTTPException(400, "Invalid address")

    try:
        res = ch().query(
            """
            SELECT
                to_address,
                count() as tx_count,
                countIf(status=1) as ok_count,
                countIf(status=0) as fail_count,
                sum(toFloat64(value_wei))/1e18 as total_eth
            FROM worldchain.world_transactions
            WHERE chain_id = 480 AND from_address = {a:FixedString(42)} AND to_address != ''
              AND to_address != '0x0000000000000000000000000000000000000000'
            GROUP BY to_address
            ORDER BY tx_count DESC
            LIMIT {lim:Int32}
            SETTINGS max_threads = 2, max_execution_time = 10, max_memory_usage = 6000000000
            """,
            parameters={"a": address, "lim": limit}
        )

        unique_addrs = {address}
        for r in res.result_rows: unique_addrs.add(normalize(r[0]).lower())
        labels_map = get_labels(list(unique_addrs))
    except Exception as e:
        raise HTTPException(500, f"ClickHouse error: {e}")

    lbl, cat = labels_map.get(address.lower(), ("", ""))
    display_label = f"{lbl} ({addr_short(address)})" if lbl else addr_short(address)
    nodes = {address: {
        "id": address, "label": display_label,
        "full": address, "type": "wallet", "category": cat or "wallet", "is_target": True
    }}
    edges = []

    for row in res.result_rows:
        to = normalize(row[0])
        count, ok, fail, eth = row[1], row[2], row[3], row[4]
        if to not in nodes:
            t_lbl, t_cat = labels_map.get(to.lower(), ("", ""))
            t_display = f"{t_lbl} ({addr_short(to)})" if t_lbl else addr_short(to)
            nodes[to] = {
                "id": to, "label": t_display,
                "full": to, "type": "contract", "category": t_cat or "contract", "is_target": False
            }
        srate = round((ok / count) * 100, 1) if count > 0 else 0
        edges.append({
            "id": f"w_{to}",
            "source": address,
            "target": to,
            "label": f"{int(count):,} txs",
            "tx_count": int(count),
            "ok_count": int(ok),
            "fail_count": int(fail),
            "eth_total": round(float(eth), 4),
            "success_rate": srate,
        })

    elements = [{"data": n} for n in nodes.values()] + [{"data": e} for e in edges]
    meta = {
        "address": address,
        "contract_interactions": len(res.result_rows)
    }
    return JSONResponse({"elements": elements, "meta": meta})


@app.get("/api/cfg/{address}")
def get_cfg_svg(address: str):
    """Return Graphviz CFG as inline SVG string."""
    address = address.lower()
    if not is_address(address):
        raise HTTPException(400, "Invalid address")

    try:
        res = ch().query(
            """
            SELECT argMax(cfg_graph, ingested_at), argMax(is_decompiled, ingested_at), argMax(is_verified, ingested_at)
            FROM worldchain.world_contracts
            WHERE chain_id = 480 AND address = {a:String}
            GROUP BY address
            LIMIT 1
            SETTINGS max_threads = 2, max_execution_time = 10
            """,
            parameters={"a": address}
        )
    except Exception as e:
        raise HTTPException(500, f"ClickHouse error: {e}")

    if not res.result_rows:
        raise HTTPException(404, "Contract not found in index")

    cfg_dot, is_decomp, is_verified = res.result_rows[0]

    if not is_decomp or not cfg_dot:
        status = "verified" if is_verified else "pending_decompilation"
        return JSONResponse({
            "status": status,
            "svg": None,
            "message": "CFG not yet generated. The decompiler is processing this contract." if not is_decomp
                       else "Contract is verified — source code available."
        })

    try:
        proc = subprocess.run(
            ["dot", "-Tsvg"],
            input=cfg_dot.encode(),
            capture_output=True,
            timeout=15
        )
        svg = proc.stdout.decode("utf-8", errors="replace")
        if not svg.strip():
            raise ValueError("Empty SVG output")
        return JSONResponse({"status": "ok", "svg": svg})
    except Exception as e:
        raise HTTPException(500, f"Graphviz render error: {e}")


@app.get("/api/stats")
def get_stats():
    """Return quick platform stats."""
    try:
        res = ch().query("""
            SELECT
                (SELECT total_rows FROM system.tables WHERE database = 'worldchain' AND name = 'world_traces') as traces,
                (SELECT count() FROM worldchain.world_contracts) as contracts,
                (SELECT countIf(is_decompiled=1) FROM worldchain.world_contracts) as decompiled,
                (SELECT total_rows FROM system.tables WHERE database = 'worldchain' AND name = 'world_contract_interactions') as interactions,
                (SELECT max(block_number) FROM worldchain.world_blocks) as latest_block
            SETTINGS max_threads = 2, max_execution_time = 10
            """)
        r = res.result_rows[0]
        return {
            "total_traces": int(r[0]),
            "total_contracts": int(r[1]),
            "decompiled_contracts": int(r[2]),
            "total_interactions": int(r[3]),
            "latest_indexed_block": int(r[4]),
        }
    except Exception as e:
        raise HTTPException(500, str(e))


@app.get("/api/mev/hourly")
def get_mev_hourly(hours: int = Query(default=24, ge=1, le=168)):
    client = ch()
    query = """
        SELECT
            toString(timestamp_hour) AS hour,
            mev_type,
            sum(total_events) AS total_events,
            round(sum(total_extracted_wei) / 1e18, 6) AS total_eth_extracted,
            round(max(max_extracted_wei) / 1e18, 6) AS peak_eth_extracted
        FROM worldchain.world_mev_hourly_stats
        WHERE timestamp_hour >= now() - INTERVAL {hours:UInt32} HOUR
        GROUP BY timestamp_hour, mev_type
        ORDER BY timestamp_hour DESC, total_events DESC
        SETTINGS max_threads = 2, max_execution_time = 10
    """
    res = client.query(query, parameters={"hours": hours})
    data = []
    for row in res.result_rows:
        data.append({
            "hour": row[0],
            "mev_type": row[1],
            "total_events": row[2],
            "total_eth_extracted": float(row[3]),
            "peak_eth_extracted": float(row[4])
        })
    return JSONResponse(content={"status": "ok", "hours": hours, "count": len(data), "data": data})


# ═════════════════════════════════════════════════════════════════════════════
# HTML FRONTEND (Embedded Dashboard v1.1)
# ═════════════════════════════════════════════════════════════════════════════

DASHBOARD_HTML = r"""<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>SGIP — Supersonic Graph Intelligence Platform</title>
<script src="https://cdnjs.cloudflare.com/ajax/libs/cytoscape/3.30.2/cytoscape.min.js"></script>
<script src="https://cdnjs.cloudflare.com/ajax/libs/dagre/0.8.5/dagre.min.js"></script>
<script src="https://cdn.jsdelivr.net/npm/cytoscape-dagre@2.5.0/cytoscape-dagre.min.js"></script>
<style>
:root {
  --bg: #0a0e1a;
  --surface: #111827;
  --surface2: #1a2234;
  --border: #1f2d45;
  --accent: #00d4ff;
  --accent2: #7c3aed;
  --green: #10b981;
  --red: #ef4444;
  --yellow: #f59e0b;
  --text: #e2e8f0;
  --muted: #64748b;
  --glow: 0 0 20px rgba(0,212,255,0.15);
}
* { box-sizing: border-box; margin: 0; padding: 0; }
body {
  font-family: 'Inter', system-ui, sans-serif;
  background: var(--bg);
  color: var(--text);
  height: 100vh;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.header {
  display: flex;
  align-items: center;
  gap: 16px;
  padding: 12px 20px;
  background: var(--surface);
  border-bottom: 1px solid var(--border);
  flex-shrink: 0;
  box-shadow: var(--glow);
}
.logo {
  font-size: 15px;
  font-weight: 800;
  letter-spacing: 2px;
  color: var(--accent);
  text-transform: uppercase;
  text-shadow: 0 0 12px rgba(0,212,255,0.4);
  cursor: pointer;
}
.logo span { color: var(--accent2); }
.badge {
  font-size: 10px;
  background: var(--accent2);
  color: #fff;
  padding: 2px 6px;
  border-radius: 4px;
  letter-spacing: 1px;
  font-weight: 700;
}

.search-wrap {
  flex: 1;
  display: flex;
  align-items: center;
  background: var(--surface2);
  border: 1px solid var(--border);
  border-radius: 8px;
  padding: 0 12px;
  transition: border-color 0.2s;
  position: relative;
}
.search-wrap input {
  background: none;
  border: none;
  outline: none;
  color: var(--text);
  font-size: 13px;
  font-family: 'JetBrains Mono', monospace;
  padding: 8px 0;
  width: 100%;
}
.search-wrap input::placeholder { color: var(--muted); }
.btn {
  background: linear-gradient(135deg, #0ea5e9, var(--accent2));
  color: #fff;
  border: none;
  border-radius: 6px;
  padding: 7px 16px;
  font-size: 12px;
  font-weight: 700;
  cursor: pointer;
  letter-spacing: 0.5px;
  transition: opacity 0.15s, transform 0.1s;
}
.btn:hover { opacity: 0.85; transform: translateY(-1px); }

.quick-links {
  display: flex;
  gap: 12px;
  margin-left: auto;
}
.q-link {
  font-size: 11px;
  color: var(--muted);
  text-decoration: none;
  padding: 4px 8px;
  border: 1px solid var(--border);
  border-radius: 4px;
  transition: all 0.2s;
}
.q-link:hover { color: var(--accent); border-color: var(--accent); }

.main {
  display: flex;
  flex: 1;
  overflow: hidden;
}

.sidebar {
  width: 300px;
  background: var(--surface);
  border-right: 1px solid var(--border);
  display: flex;
  flex-direction: column;
  overflow: hidden;
  flex-shrink: 0;
}
.tabs {
  display: flex;
  border-bottom: 1px solid var(--border);
  flex-shrink: 0;
  background: var(--surface2);
}
.tab {
  flex: 1;
  padding: 10px 14px;
  font-size: 11px;
  font-weight: 600;
  color: var(--muted);
  cursor: pointer;
  border-bottom: 2px solid transparent;
  text-align: center;
  transition: all 0.15s;
}
.tab.active { color: var(--accent); border-bottom-color: var(--accent); background: var(--surface); }

.detail-content {
  flex: 1;
  overflow-y: auto;
  padding: 16px;
  font-size: 12px;
}
.panel-title {
  font-size: 10px;
  font-weight: 700;
  letter-spacing: 1.5px;
  text-transform: uppercase;
  color: var(--muted);
  margin-bottom: 12px;
}

.detail-row {
  display: flex;
  justify-content: space-between;
  align-items: flex-start;
  padding: 8px 0;
  border-bottom: 1px solid var(--border);
  gap: 8px;
}
.detail-label { color: var(--muted); flex-shrink: 0; }
.detail-val {
  color: var(--text);
  font-family: 'JetBrains Mono', monospace;
  font-size: 11px;
  word-break: break-all;
  text-align: right;
}
.detail-val.addr {
  color: var(--accent);
  cursor: pointer;
  text-decoration: underline dotted;
}
.detail-val.addr:hover { color: #fff; }

.leaderboard-table {
  width: 100%;
  border-collapse: collapse;
  margin-top: 8px;
}
.leaderboard-table th, .leaderboard-table td {
  padding: 8px;
  text-align: left;
  border-bottom: 1px solid var(--border);
  font-size: 11px;
}
.leaderboard-table th { color: var(--muted); font-weight: 600; }
.leaderboard-table tr:hover { background: rgba(255,255,255,0.02); cursor: pointer; }
.badge-cat {
  font-size: 9px;
  padding: 2px 4px;
  border-radius: 3px;
  background: var(--surface2);
  color: var(--accent);
}

.graph-area {
  flex: 1;
  position: relative;
  overflow: hidden;
}
#cy {
  width: 100%;
  height: 100%;
  background: var(--bg);
}
#cfg-area {
  width: 100%;
  height: 100%;
  overflow: auto;
  padding: 16px;
  display: none;
}

.graph-controls {
  position: absolute;
  bottom: 16px;
  right: 16px;
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.ctrl-btn {
  width: 32px;
  height: 32px;
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: 6px;
  color: var(--text);
  font-size: 16px;
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  transition: all 0.15s;
}
.ctrl-btn:hover { border-color: var(--accent); color: var(--accent); }

.legend {
  position: absolute;
  top: 12px;
  left: 12px;
  background: rgba(17,24,39,0.9);
  border: 1px solid var(--border);
  border-radius: 8px;
  padding: 10px 12px;
  font-size: 11px;
  backdrop-filter: blur(4px);
  display: none;
  z-index: 10;
}
.legend.visible { display: block; }
.legend-item { display: flex; align-items: center; gap: 7px; margin: 3px 0; }
.legend-dot { width: 10px; height: 10px; border-radius: 50%; }

.suggestions-box {
  position: absolute;
  top: 100%;
  left: 0;
  right: 0;
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: 0 0 8px 8px;
  max-height: 200px;
  overflow-y: auto;
  z-index: 100;
  display: none;
}
.suggestion-item {
  padding: 8px 12px;
  font-size: 12px;
  cursor: pointer;
  border-bottom: 1px solid var(--border);
}
.suggestion-item:hover { background: var(--surface2); color: var(--accent); }

.overlay {
  position: absolute;
  inset: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  color: var(--muted);
  font-size: 13px;
  gap: 16px;
}
.overlay.hidden { display: none; }
.spinner {
  width: 36px;
  height: 36px;
  border: 3px solid var(--border);
  border-top-color: var(--accent);
  border-radius: 50%;
  animation: spin 0.8s linear infinite;
}
@keyframes spin { to { transform: rotate(360deg); } }

.toast {
  position: fixed;
  bottom: 20px;
  left: 50%;
  transform: translateX(-50%) translateY(80px);
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: 8px;
  padding: 10px 18px;
  font-size: 12px;
  transition: transform 0.3s;
  z-index: 999;
}
.toast.show { transform: translateX(-50%) translateY(0); }
.toast.ok { border-color: var(--green); color: var(--green); }
.toast.error { border-color: var(--red); color: var(--red); }

.mode-btns { display: flex; gap: 6px; }
.mode-btn {
  background: var(--surface2);
  border: 1px solid var(--border);
  color: var(--muted);
  border-radius: 6px;
  padding: 6px 12px;
  font-size: 11px;
  font-weight: 600;
  cursor: pointer;
  transition: all 0.15s;
}
.mode-btn.active { background: rgba(0,212,255,0.1); border-color: var(--accent); color: var(--accent); }

body.embed-mode .header { display: none; }
</style>
</head>
<body>

<header class="header">
  <div class="logo" onclick="showHome()">SGI<span>P</span></div>
  <div class="badge">WORLD CHAIN</div>
  <div class="search-wrap">
    <input id="search-input" type="text" placeholder="Paste hash, address, block, or type protocol name..." autocomplete="off" spellcheck="false"/>
    <div class="suggestions-box" id="suggestions"></div>
  </div>
  <div class="mode-btns">
    <button class="mode-btn active" onclick="setMode('contract')" id="mb-contract">⬡ Contract</button>
    <button class="mode-btn" onclick="setMode('tx')" id="mb-tx">⬢ Transaction</button>
    <button class="mode-btn" onclick="setMode('wallet')" id="mb-wallet">◉ Wallet</button>
    <button class="mode-btn" onclick="setMode('cfg')" id="mb-cfg">⬡ CFG</button>
  </div>
  <button class="btn" onclick="doVisualize()">VISUALIZE ⚡</button>

</header>

<div class="main">
  <aside class="sidebar">
    <div class="tabs">
      <div class="tab active" onclick="switchTab('details')" id="tab-details">Details</div>
      <div class="tab" onclick="switchTab('meta')" id="tab-meta">Meta / Stats</div>
    </div>
    <div class="detail-content" id="sidebar-content">
      <!-- Content populated dynamically -->
    </div>
  </aside>

  <div class="graph-area">
    <div id="cy"></div>
    <div id="cfg-area"></div>

    <div class="overlay" id="overlay-empty">
      <div style="font-size: 24px;">⚡ Supersonic Leaderboard</div>
      <div style="width: 90%; max-height: 80%; overflow-y: auto;">
        <table class="leaderboard-table" id="leaderboard-body">
          <!-- Populated by script -->
        </table>
      </div>
    </div>

    <div class="overlay hidden" id="overlay-loading">
      <div class="spinner"></div>
      <div>Querying World Chain ClickHouse index...</div>
    </div>

    <div class="legend" id="legend">
      <div class="legend-item"><div class="legend-dot" style="background:#00d4ff"></div>Target / Center</div>
      <div class="legend-item"><div class="legend-dot" style="background:#10b981"></div>Caller / Success</div>
      <div class="legend-item"><div class="legend-dot" style="background:#7c3aed"></div>Callee</div>
      <div class="legend-item"><div class="legend-dot" style="background:#ef4444"></div>Error / Revert</div>
    </div>

    <div class="graph-controls">
      <button class="ctrl-btn" onclick="cy&&cy.fit()">⊙</button>
      <button class="ctrl-btn" onclick="cy&&cy.zoom(cy.zoom()*1.2)">+</button>
      <button class="ctrl-btn" onclick="cy&&cy.zoom(cy.zoom()*0.8)">-</button>
    </div>
  </div>
</div>

<div class="toast" id="toast"></div>

<script>
let cy = null;
let currentMode = 'contract';

const params = new URLSearchParams(location.search);
if (params.has('embed')) document.body.classList.add('embed-mode');

['tx','contract','wallet','cfg'].forEach(t => {
  if (params.has(t)) {
    setMode(t);
    document.getElementById('search-input').value = params.get(t);
    setTimeout(doVisualize, 200);
  }
});

loadLeaderboard();

function setMode(m) {
  currentMode = m;
  ['contract','tx','wallet','cfg'].forEach(x => {
    document.getElementById('mb-'+x).classList.toggle('active', x===m);
  });
}

function showHome() {
  document.getElementById('overlay-empty').classList.remove('hidden');
  document.getElementById('cy').style.display = 'none';
  document.getElementById('cfg-area').style.display = 'none';
  document.getElementById('legend').classList.remove('visible');
  loadLeaderboard();
}

async function loadLeaderboard() {
  try {
    const r = await fetch('/api/top');
    if (!r.ok) return;
    const data = await r.json();
    let html = `<tr><th>Rank</th><th>Contract Label / Address</th><th>Category</th><th>Calls</th></tr>`;
    data.forEach((x, i) => {
      html += `<tr onclick="exploreAddress('${x.address}')">
        <td>#${i+1}</td>
        <td style="font-family: monospace;">${x.label}</td>
        <td><span class="badge-cat">${x.category}</span></td>
        <td>${x.total_calls.toLocaleString()}</td>
      </tr>`;
    });
    document.getElementById('leaderboard-body').innerHTML = html;
  } catch(e) {}
}

function exploreAddress(addr) {
  document.getElementById('search-input').value = addr;
  setMode('contract');
  doVisualize();
}

const searchInput = document.getElementById('search-input');
const suggestionsBox = document.getElementById('suggestions');

searchInput.addEventListener('input', async () => {
  const val = searchInput.value.trim();
  if (val.length < 3) { suggestionsBox.style.display = 'none'; return; }
  try {
    const r = await fetch(`/api/search?q=${val}`);
    const data = await r.json();
    if (data.type === 'suggestions' && data.value.length > 0) {
      suggestionsBox.innerHTML = data.value.map(x =>
        `<div class="suggestion-item" onclick="selectSuggestion('${x.address}')">${x.label} (${x.address.slice(0,8)}...)</div>`
      ).join('');
      suggestionsBox.style.display = 'block';
    } else {
      suggestionsBox.style.display = 'none';
    }
  } catch(e) {}
});

function selectSuggestion(addr) {
  searchInput.value = addr;
  suggestionsBox.style.display = 'none';
  setMode('contract');
  doVisualize();
}

async function doVisualize() {
  const q = searchInput.value.trim();
  if (!q) return;
  suggestionsBox.style.display = 'none';

  if (/^\d+$/.test(q)) {
    loadBlockExplorer(q);
    return;
  }

  showLoading(true);
  document.getElementById('overlay-empty').classList.add('hidden');

  try {
    if (currentMode === 'cfg') {
      await loadCFG(q);
    } else {
      const url = `/api/${currentMode}/${q}`;
      const r = await fetch(url);
      if (!r.ok) throw new Error(await r.text());
      const data = await r.json();

      document.getElementById('cy').style.display = 'block';
      document.getElementById('cfg-area').style.display = 'none';
      document.getElementById('legend').classList.add('visible');

      renderGraph(data.elements);
      showMeta(data.meta);
    }
  } catch(e) {
    showToast(e.message || 'Error fetching graph', 'error');
  } finally {
    showLoading(false);
  }
}

async function loadBlockExplorer(blockNum) {
  showLoading(true);
  try {
    const r = await fetch(`/api/block/${blockNum}`);
    if (!r.ok) throw new Error(await r.text());
    const data = await r.json();

    document.getElementById('cy').style.display = 'none';
    document.getElementById('cfg-area').style.display = 'block';
    document.getElementById('legend').classList.remove('visible');

    let html = `<div style="padding:16px;">
      <h2>Block #${blockNum} Transactions</h2>
      <table class="leaderboard-table">
        <tr><th>Hash</th><th>From</th><th>To</th><th>Value (ETH)</th><th>Status</th></tr>`;
    data.transactions.forEach(tx => {
      html += `<tr>
        <td style="color:var(--accent); cursor:pointer;" onclick="viewTx('${tx.hash}')">${tx.hash.slice(0,12)}...</td>
        <td>${tx.from_label}</td>
        <td>${tx.to_label}</td>
        <td>${tx.value_eth}</td>
        <td style="color:${tx.status === 1 ? 'var(--green)' : 'var(--red)'}">${tx.status === 1 ? 'Success' : 'Revert'}</td>
      </tr>`;
    });
    html += `</table></div>`;
    document.getElementById('cfg-area').innerHTML = html;
  } catch(e) {
    showToast('Block not found or empty', 'error');
  } finally {
    showLoading(false);
  }
}

function viewTx(hash) {
  searchInput.value = hash;
  setMode('tx');
  doVisualize();
}

async function loadCFG(addr) {
  const r = await fetch(`/api/cfg/${addr}`);
  const data = await r.json();
  document.getElementById('cy').style.display = 'none';
  document.getElementById('cfg-area').style.display = 'block';
  if (!data.svg) {
    document.getElementById('cfg-area').innerHTML = `<div style="padding:20px;color:var(--muted);">${data.message}</div>`;
  } else {
    document.getElementById('cfg-area').innerHTML = data.svg;
  }
}

function renderGraph(elements) {
  if (cy) cy.destroy();
  cy = cytoscape({
    container: document.getElementById('cy'),
    elements: elements,
    style: [
      {
        selector: 'node',
        style: {
          'label': 'data(label)',
          'color': '#e2e8f0',
          'font-size': '10px',
          'background-color': '#111827',
          'border-color': '#1f2d45',
          'border-width': 2,
          'width': 40,
          'height': 40,
          'text-valign': 'bottom',
          'text-margin-y': 4
        }
      },
      {
        selector: 'node[is_target]',
        style: {
          'border-color': '#00d4ff',
          'background-color': 'rgba(0, 212, 255, 0.2)',
          'width': 50,
          'height': 50
        }
      },
      {
        selector: 'node[role="caller"]',
        style: { 'border-color': '#10b981' }
      },
      {
        selector: 'node[role="callee"]',
        style: { 'border-color': '#7c3aed' }
      },
      {
        selector: 'node[has_error]',
        style: { 'border-color': '#ef4444' }
      },
      {
        selector: 'edge',
        style: {
          'label': 'data(label)',
          'font-size': '8px',
          'color': '#64748b',
          'curve-style': 'bezier',
          'target-arrow-shape': 'triangle',
          'line-color': '#1f2d45',
          'target-arrow-color': '#1f2d45',
          'width': 1.5
        }
      },
      {
        selector: 'edge[direction="in"]',
        style: { 'line-color': 'rgba(16, 185, 129, 0.3)', 'target-arrow-color': '#10b981' }
      },
      {
        selector: 'edge[direction="out"]',
        style: { 'line-color': 'rgba(124, 58, 237, 0.3)', 'target-arrow-color': '#7c3aed' }
      },
      {
        selector: 'edge[error]',
        style: { 'line-color': 'rgba(239, 68, 68, 0.5)', 'target-arrow-color': '#ef4444' }
      }
    ],
    layout: {
      name: currentMode === 'tx' ? 'dagre' : 'concentric',
      fit: true,
      padding: 30
    }
  });

  cy.on('tap', 'node', e => showNodeDetails(e.target.data()));
  cy.on('tap', 'edge', e => showEdgeDetails(e.target.data()));
}

function showNodeDetails(data) {
  let html = `<div class="panel-title">Node Details</div>`;
  html += `<div class="detail-row"><span class="detail-label">Address</span><span class="detail-val addr" onclick="navigator.clipboard.writeText('${data.full}');showToast('Copied!')">${data.full}</span></div>`;
  if (data.category) html += `<div class="detail-row"><span class="detail-label">Category</span><span class="detail-val">${data.category}</span></div>`;
  if (data.depth !== undefined) html += `<div class="detail-row"><span class="detail-label">Trace Depth</span><span class="detail-val">${data.depth}</span></div>`;
  document.getElementById('sidebar-content').innerHTML = html;
}

function showEdgeDetails(data) {
  let html = `<div class="panel-title">Call Details</div>`;
  if (data.decoded_signature) html += `<div class="detail-row"><span class="detail-label">Decoded ABI</span><span class="detail-val" style="color:var(--green);">${data.decoded_signature}</span></div>`;
  if (data.selector) html += `<div class="detail-row"><span class="detail-label">Method Selector</span><span class="detail-val">${data.selector}</span></div>`;
  if (data.calls) html += `<div class="detail-row"><span class="detail-label">Call Count</span><span class="detail-val">${data.calls.toLocaleString()}</span></div>`;
  if (data.errors) html += `<div class="detail-row"><span class="detail-label">Errors</span><span class="detail-val" style="color:var(--red);">${data.errors} (${data.err_pct}%)</span></div>`;
  if (data.eth_value !== undefined && data.eth_value > 0) html += `<div class="detail-row"><span class="detail-label">Value</span><span class="detail-val">${data.eth_value} ETH</span></div>`;
  if (data.trace_type) html += `<div class="detail-row"><span class="detail-label">Call Type</span><span class="detail-val">${data.trace_type.toUpperCase()}</span></div>`;
  if (data.error) html += `<div class="detail-row"><span class="detail-label">Revert Reason</span><span class="detail-val" style="color:var(--red);">${data.error}</span></div>`;
  document.getElementById('sidebar-content').innerHTML = html;
}

function showMeta(meta) {
  window.currentMeta = meta;
}

function switchTab(tab) {
  document.getElementById('tab-details').classList.toggle('active', tab==='details');
  document.getElementById('tab-meta').classList.toggle('active', tab==='meta');

  if (tab === 'meta') {
    if (!window.currentMeta) {
      document.getElementById('sidebar-content').innerHTML = '<div style="color:var(--muted);">No metadata loaded.</div>';
      return;
    }
    let html = `<div class="panel-title">Graph Metadata</div>`;
    Object.entries(window.currentMeta).forEach(([k, v]) => {
      html += `<div class="detail-row"><span class="detail-label">${k.replace(/_/g, ' ')}</span><span class="detail-val">${v}</span></div>`;
    });
    document.getElementById('sidebar-content').innerHTML = html;
  }
}

function showLoading(v) {
  document.getElementById('overlay-loading').classList.toggle('hidden', !v);
}

let toastTimer;
function showToast(msg, type='ok') {
  const t = document.getElementById('toast');
  t.textContent = msg;
  t.className = `toast show ${type}`;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => t.classList.remove('show'), 3000);
}
</script>
</body>
</html>"""

@app.get("/")
def dashboard():
    return HTMLResponse(DASHBOARD_HTML)

@app.get("/embed")
def embed():
    return HTMLResponse(DASHBOARD_HTML.replace('<title>SGIP', '<title>SGIP Embed'))
