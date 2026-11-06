/**
 * Synthetic test fixtures — obviously FAKE, never live evidence.
 */

import type { PaymentPayload, SellerRequest } from "../src/types.js";
import { SellerEngine } from "../src/seller.js";
import { FakeGatewayClient } from "../src/gateway/fakeGateway.js";
import { FakeUpstreamClient } from "../src/upstream/fakeUpstream.js";
import { MemoryLedger } from "../src/ledger/memoryLedger.js";
import { ARC_TESTNET_GATEWAY_WALLET } from "../src/chainConfig.js";

export const FIXTURE_NETWORK = "eip155:5042002";
export const FIXTURE_ASSET = "0x3600000000000000000000000000000000000000";
export const FIXTURE_PAY_TO = "0x1111111111111111111111111111111111111111";
export const FIXTURE_PAYER = "0x2222222222222222222222222222222222222222";
export const FIXTURE_VERIFYING = ARC_TESTNET_GATEWAY_WALLET;

/** 32-byte nonce as 0x + 64 hex — synthetic. */
export function fakeNonce(seed = 1): string {
  return `0x${seed.toString(16).padStart(64, "0")}`;
}

/**
 * Canonical x402 v2 payment fixture. Selected terms live in `accepted`.
 * Default validity window is far enough ahead of the fixture clock for Circle's
 * 3-day minimum (validBefore - nowSec >= 259200).
 */
export function makePayment(overrides: {
  from?: string;
  to?: string;
  value?: string;
  validAfter?: number;
  validBefore?: number;
  nonce?: string;
  network?: string;
  asset?: string;
  scheme?: string;
  verifyingContract?: string;
  signature?: string;
  x402Version?: number;
  omitAccepted?: boolean;
  acceptedAmount?: string;
  acceptedPayTo?: string;
} = {}): PaymentPayload {
  const validAfter = overrides.validAfter ?? 1_700_000_000;
  // Default: ~1157 days ahead of fixture now (1_800_000_000) — satisfies 3-day rule
  const validBefore = overrides.validBefore ?? 1_900_000_000;
  const network = overrides.network ?? FIXTURE_NETWORK;
  const asset = overrides.asset ?? FIXTURE_ASSET;
  const amount = overrides.value ?? "10000";
  const payTo = overrides.to ?? FIXTURE_PAY_TO;
  const payment = {
    x402Version: (overrides.x402Version ?? 2) as 2,
    accepted: {
      scheme: overrides.scheme ?? "exact",
      network,
      asset,
      amount: overrides.acceptedAmount ?? amount,
      payTo: overrides.acceptedPayTo ?? payTo,
      maxTimeoutSeconds: 345600,
      extra: {
        name: "GatewayWalletBatched" as const,
        version: "1",
        verifyingContract: overrides.verifyingContract ?? FIXTURE_VERIFYING,
      },
    },
    payload: {
      signature: overrides.signature ?? "0xFAKE_SYNTHETIC_SIGNATURE_NOT_A_REAL_SIG",
      authorization: {
        from: overrides.from ?? FIXTURE_PAYER,
        to: payTo,
        value: amount,
        validAfter,
        validBefore,
        nonce: overrides.nonce ?? fakeNonce(1),
      },
    },
  };
  if (overrides.omitAccepted) {
    delete (payment as { accepted?: unknown }).accepted;
  }
  return payment as PaymentPayload;
}

export function makeRequest(overrides: Partial<SellerRequest> & { path?: string } = {}): SellerRequest {
  return {
    method: overrides.method ?? "GET",
    path: overrides.path ?? "/v1/balance",
    rawQuery:
      overrides.rawQuery ??
      "address=0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
    bodyByteLength: overrides.bodyByteLength ?? 0,
    payment: overrides.payment === undefined ? null : overrides.payment,
  };
}

export function makeSeller(opts: {
  gateway?: FakeGatewayClient;
  upstream?: FakeUpstreamClient;
  ledger?: MemoryLedger;
  nowMs?: number;
  fulfillmentRetryBudget?: number;
  fulfillmentRetryBudgetMs?: number;
  leaseMs?: number;
  leaseWaitMs?: number;
} = {}) {
  let nowMs = opts.nowMs ?? 1_800_000_000_000; // within default validity window
  const gateway = opts.gateway ?? new FakeGatewayClient({ settleMode: { kind: "success" } });
  const upstream = opts.upstream ?? new FakeUpstreamClient({ kind: "success" });
  const ledger = opts.ledger ?? new MemoryLedger();
  const engine = new SellerEngine({
    gateway,
    upstream,
    ledger,
    config: {
      network: FIXTURE_NETWORK,
      asset: FIXTURE_ASSET,
      payTo: FIXTURE_PAY_TO,
      verifyingContract: FIXTURE_VERIFYING,
      clock: () => new Date(nowMs),
      fulfillmentRetryBudget: opts.fulfillmentRetryBudget ?? 2,
      fulfillmentRetryBudgetMs: opts.fulfillmentRetryBudgetMs ?? 60_000,
      attemptCap: 3,
      leaseMs: opts.leaseMs ?? 1_000,
      leaseWaitMs: opts.leaseWaitMs ?? 500,
    },
  });
  return {
    engine,
    gateway,
    upstream,
    ledger,
    setNowMs(ms: number) {
      nowMs = ms;
    },
    advanceMs(delta: number) {
      nowMs += delta;
    },
  };
}
