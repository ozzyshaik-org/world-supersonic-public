#!/usr/bin/env python3
import logging
import os
import json
import threading
import urllib.request

log = logging.getLogger("mev")

DISCORD_WEBHOOK_URL = os.getenv("DISCORD_WEBHOOK_URL", "")
TELEGRAM_BOT_TOKEN = os.getenv("TELEGRAM_BOT_TOKEN", "")
TELEGRAM_CHAT_ID = os.getenv("TELEGRAM_CHAT_ID", "")

def _dispatch_webhooks_async(tx_hash: str, eth_val: float, profit_wei: int, mev_type: str, enriched_text: str = None):
    msg = enriched_text if enriched_text else (
        f"⚡ **WORLDCHAIN HIGH-VALUE MEV DETECTED**\n"
        f"• **Type:** `{mev_type.upper()}`\n"
        f"• **Extracted:** `{eth_val} ETH` ({profit_wei} wei)\n"
        f"• **Tx Hash:** `{tx_hash.strip()}`"
    )
    
    # 1. Discord Webhook
    if DISCORD_WEBHOOK_URL:
        try:
            payload = json.dumps({"content": msg}).encode("utf-8")
            req = urllib.request.Request(
                DISCORD_WEBHOOK_URL,
                data=payload,
                headers={"Content-Type": "application/json", "User-Agent": "Supersonic-MEV/1.0"}
            )
            urllib.request.urlopen(req, timeout=5)
        except Exception as e:
            log.error(f"[ALERT-HOOK] Discord alert failed: {e}")

    # 2. Telegram Bot Webhook
    if TELEGRAM_BOT_TOKEN and TELEGRAM_CHAT_ID:
        try:
            tg_url = f"https://api.telegram.org/bot{TELEGRAM_BOT_TOKEN}/sendMessage"
            tg_payload = json.dumps({
                "chat_id": TELEGRAM_CHAT_ID,
                "text": msg,
                "parse_mode": "Markdown"
            }).encode("utf-8")
            req = urllib.request.Request(
                tg_url,
                data=tg_payload,
                headers={"Content-Type": "application/json"}
            )
            urllib.request.urlopen(req, timeout=5)
        except Exception as e:
            log.error(f"[ALERT-HOOK] Telegram alert failed: {e}")

def trigger_alert(tx_hash, profit_wei: int, mev_type: str = "atomic_arbitrage", enriched_text: str = None):
    if isinstance(tx_hash, bytes):
        tx_hash = tx_hash.decode("utf-8", errors="ignore")
    eth_val = round(profit_wei / 1e18, 6)
    log_msg = f"🔥 HIGH-VALUE MEV DETECTED [{mev_type.upper()}] | Tx: {tx_hash.strip()} | Profit: {eth_val} ETH ({profit_wei} wei)"
    log.info(log_msg)
    
    t = threading.Thread(
        target=_dispatch_webhooks_async,
        args=(tx_hash, eth_val, profit_wei, mev_type, enriched_text),
        daemon=True
    )
    t.start()

if __name__ == "__main__":
    print("Testing trigger_alert async dispatch to Telegram...")
    trigger_alert("0xe31e51ca30bf25682c1133b38ff3d3e2fb58596af7d45ec5d5dcba5ca950d94c", 100000000000000000, "atomic_arbitrage")
    print("[OK] Test alert queued for dispatch.")
