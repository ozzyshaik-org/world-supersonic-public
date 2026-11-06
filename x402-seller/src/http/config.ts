/**
 * HTTP adapter config from environment only.
 * Never commit real credentials, hostnames of live infra, or IPs of private hosts.
 */

import { ARC_TESTNET_CAIP2, ARC_TESTNET_GATEWAY_WALLET } from "../chainConfig.js";

export interface AdapterEnvConfig {
  listenHost: string;
  listenPort: number;
  network: string;
  asset: string;
  payTo: string;
  verifyingContract: string;
  upstreamBaseUrl: string;
  upstreamApiKey: string;
  upstreamTimeoutMs: number;
  settleTimeoutMs: number;
  unresolvedRetryAfterSeconds: number;
  /** Sole allowlisted paid route exposed by this thin adapter. */
  allowlistedPath: string;
}

const DEFAULT_ALLOWLISTED_PATH = "/v1/mev/analytics";

function readRequired(env: NodeJS.ProcessEnv, name: string): string {
  const v = env[name];
  if (v === undefined || v.length === 0) {
    throw new Error(`missing required env ${name}`);
  }
  return v;
}

function readInt(env: NodeJS.ProcessEnv, name: string, fallback: number): number {
  const raw = env[name];
  if (raw === undefined || raw.length === 0) return fallback;
  const n = Number.parseInt(raw, 10);
  if (!Number.isFinite(n) || n < 0) {
    throw new Error(`invalid integer env ${name}`);
  }
  return n;
}

/**
 * Load adapter config. Live Circle API keys are intentionally not read here —
 * this adapter is for local/fake Gateway wiring until credentials exist.
 */
export function loadAdapterConfigFromEnv(
  env: NodeJS.ProcessEnv = process.env,
): AdapterEnvConfig {
  return {
    listenHost: env.X402_LISTEN_HOST ?? "127.0.0.1",
    listenPort: readInt(env, "X402_LISTEN_PORT", 0),
    network: env.X402_NETWORK ?? ARC_TESTNET_CAIP2,
    asset: readRequired(env, "X402_USDC_ASSET"),
    payTo: readRequired(env, "X402_PAY_TO"),
    verifyingContract: env.X402_GATEWAY_WALLET ?? ARC_TESTNET_GATEWAY_WALLET,
    upstreamBaseUrl: readRequired(env, "X402_UPSTREAM_BASE_URL"),
    upstreamApiKey: readRequired(env, "X402_UPSTREAM_API_KEY"),
    upstreamTimeoutMs: readInt(env, "X402_UPSTREAM_TIMEOUT_MS", 5_000),
    settleTimeoutMs: readInt(env, "X402_SETTLE_TIMEOUT_MS", 5_000),
    unresolvedRetryAfterSeconds: readInt(env, "X402_UNRESOLVED_RETRY_AFTER_SECONDS", 2),
    allowlistedPath: env.X402_ALLOWLISTED_PATH ?? DEFAULT_ALLOWLISTED_PATH,
  };
}
