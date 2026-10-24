#!/usr/bin/env python3
"""MEV alert enrichment - CH-only queries, primary-key aligned."""
import json
import sys
import urllib.request

import os


import clickhouse_connect
_ch = clickhouse_connect.get_client(host="127.0.0.1", port=8123, database="worldchain")

def q(sql, params=None):
    return _ch.query(sql, parameters=params or {}).result_rows


def hstr(v):
    if isinstance(v, bytes):
        return v.decode(errors='replace').split('\x00')[0].strip()
    return str(v).split('\x00')[0].strip()


def get_eth_price():
    r = q("SELECT price_usd FROM worldchain.latest_oracle_prices "
          "WHERE token_symbol = 'ETH' ORDER BY updated_at DESC LIMIT 1")
    return float(r[0][0]) if r else 0.0


def get_token_transfers(tx_hash, block_number):
    r = q("SELECT lower(address), topics[3], substring(data, 1, 64) "
          "FROM worldchain.world_logs "
          "WHERE chain_id = 480 AND block_number = {b:UInt64} "
          "AND tx_hash = {h:String} "
          "AND topics[1] = '0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef' "
          "ORDER BY log_index",
          {'h': tx_hash, 'b': block_number})
    out = []
    for tok, to_topic, amt_hex in r:
        tok = hstr(tok).lower()
        to = '0x' + hstr(to_topic)[-40:]
        d = (amt_hex if isinstance(amt_hex, str) else bytes(amt_hex).hex())
        d = d.removeprefix('0x')
        amt = int(d, 16) if d else 0
        sym = {'0x4200000000000000000000000000000000000006': 'WETH'}.get(tok)
        out.append({'token': tok, 'symbol': sym, 'amount': amt, 'to': to})
    return out


def enrich(tx_hash, block_number, net_wei, gas_wei, pools):
    eth_price = get_eth_price()
    net_eth = int(net_wei) / 1e18
    gas_eth = int(gas_wei) / 1e18
    usd = net_eth * eth_price if eth_price else 0.0
    transfers = get_token_transfers(tx_hash, block_number)
    toks = []
    for t in transfers[:6]:
        sym = t['symbol'] or t['token'][:10] + '...'
        amt = t['amount'] / 1e18 if t['amount'] > 10**12 else t['amount']
        toks.append(f'{sym}: {amt:.6f}' if t['amount'] > 10**12 else f'{sym}: {amt}')
    tok_str = ' | '.join(toks) if toks else 'none'
    pool_str = ' | '.join(p[:14] + '...' for p in (pools or [])[:4]) or 'none'
    protocol = 'Uniswap V3' if pools else ''
    es = f'https://worldscan.org/tx/{tx_hash}'
    bs = f'https://worldchain-mainnet.explorer.alchemy.com/tx/{tx_hash}'
    if net_eth >= 0.001:
        eth_str = f'{net_eth:.6f} ETH'
    elif net_eth > 0:
        eth_str = f'{net_eth:.10f} ETH'
    else:
        eth_str = f'{net_eth} ETH'
    lines = [
        'MEV ARBITRAGE - World Chain',
        f'Net Profit: {eth_str} ({net_wei} wei)',
        f'USD: ~${usd:.4f}' if usd else 'USD: n/a',
        f'Gas: {gas_eth:.8f} ETH',
        f'Tokens: {tok_str}',
        f'Protocol: {protocol}' if protocol else f'Pools: {pool_str}',
        f'Etherscan: {es}',
        f'Blockscout: {bs}',
    ]
    meta = {'net_eth': net_eth, 'gas_eth': gas_eth, 'usd': usd,
            'tokens': toks, 'pools': pools, 'eth_price': eth_price}
    return '\n'.join(lines), meta


if __name__ == '__main__':
    tx = '0x45489130a88944125728eb620e403e99a0317c4f6f41dd6b9dbcec720697f04c'
    text, meta = enrich(tx, 34349287, 1397642765731132837, 1585800530000, [])
    print(text)
    print(json.dumps(meta, indent=1))
