/**
 * Synthetic test fixtures — obviously FAKE, never live evidence.
 */

import type { PaymentPayload, SellerRequest } from "../src/types.js";
import { SellerEngine } from "../src/seller.js";
import { FakeGatewayClient } from "../src/gateway/fakeGateway.js";
import { FakeUpstreamClient } from "../src/upstream/fakeUpstream.js";
import { MemoryLedger } from "../src/ledger/memoryLedger.js";

export const FIXTURE_NETWORK = "eip155:5042002";
export const FIXTURE_ASSET = "0x3600000000000000000000000000000000000000";
export const FIXTURE_PAY_TO = "0x1111111111111111111111111111111111111111";
export const FIXTURE_PAYER = "0x2222222222222222222222222222222222222222";

/** 32-byte nonce as 0x + 64 hex — synthetic. */
export function fakeNonce(seed = 1): string {
  return `0x${seed.toString(16).padStart(64, "0")}`;
}

export function makePayment(overrides: {
  from?: string;
  to?: string;
  value?: string;
  validAfter?: number;
  validBefore?: number;
  nonce?: string;
  network?: string;
} = {}): PaymentPayload {
  const validAfter = overrides.validAfter ?? 1_700_000_000;
  const validBefore = overrides.validBefore ?? 1_900_000_000;
  return {
    x402Version: 2,
    scheme: "exact",
    network: overrides.network ?? FIXTURE_NETWORK,
    payload: {
      signature: "0xFAKE_SYNTHETIC_SIGNATURE_NOT_A_REAL_SIG",
      authorization: {
        from: overrides.from ?? FIXTURE_PAYER,
        to: overrides.to ?? FIXTURE_PAY_TO,
        value: overrides.value ?? "10000",
        validAfter,
        validBefore,
        nonce: overrides.nonce ?? fakeNonce(1),
      },
    },
  };
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
      clock: () => new Date(nowMs),
      fulfillmentRetryBudget: 2,
      attemptCap: 3,
      leaseMs: 1_000,
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
