/**
 * Deterministic interleaving for the confirmed stale-worker budget race.
 * Barriers use deferred promises only — no sleep-based racing.
 *
 * Confirmed race (pre-fix):
 *   markRetryableFailure() releases the lease → peer takeLease (newer generation)
 *   → unfenced markFailedPermanent() overwrites peer in_progress
 *   → peer 2xx commit rejected → paid_unfulfilled.
 */
import { describe, expect, it } from "vitest";
import { MemoryLedger } from "../src/ledger/memoryLedger.js";
import { SellerEngine } from "../src/seller.js";
import { FakeGatewayClient } from "../src/gateway/fakeGateway.js";
import { FakeUpstreamClient } from "../src/upstream/fakeUpstream.js";
import type { LedgerRow } from "../src/types.js";
import {
  FIXTURE_ASSET,
  FIXTURE_NETWORK,
  FIXTURE_PAY_TO,
  FIXTURE_VERIFYING,
  fakeNonce,
  makePayment,
  makeRequest,
  makeSeller,
} from "./helpers.js";

function deferred<T = void>(): {
  promise: Promise<T>;
  resolve: (v: T | PromiseLike<T>) => void;
} {
  let resolve!: (v: T | PromiseLike<T>) => void;
  const promise = new Promise<T>((r) => {
    resolve = r;
  });
  return { promise, resolve };
}

function internalRow(ledger: MemoryLedger, authKey: string): LedgerRow {
  return (
    ledger as unknown as { byAuthKey: Map<string, LedgerRow> }
  ).byAuthKey.get(authKey)!;
}

describe("stale markFailedPermanent after markRetryableFailure release (confirmed race)", () => {
  it("ledger interleaving: unfenced permanent after peer takeLease must not clobber; peer 2xx commits", async () => {
    const gateway = new FakeGatewayClient({ settleMode: { kind: "success" } });
    const upstream = new FakeUpstreamClient({ kind: "success" });
    const { engine, ledger } = makeSeller({ gateway, upstream });
    const nonce = fakeNonce(501);
    const payment = makePayment({ nonce });
    await engine.handle(makeRequest({ payment }));

    const row0 = ledger.getByNonce(nonce)!;
    const row = internalRow(ledger, row0.authKey);
    row.fulfillmentState = "in_progress";
    row.storedResponse = null;
    row.receipt = null;
    row.leaseOwner = "stale-owner";
    row.leaseGeneration = 1;
    row.leaseExpiresAt = Date.now() + 60_000;
    row.fulfillmentAttemptCount = 1;
    row.fulfillmentFirstAttemptAt = new Date().toISOString();
    row.incidents = [];

    const nowMs = Date.now();
    const nowIso = new Date().toISOString();

    const released = ledger.markRetryableFailure(
      row0.authKey,
      "stale-owner",
      1,
      nowMs,
      nowIso,
    );
    expect(released).not.toBeNull();
    expect(released!.fulfillmentState).toBe("retryable_failure");
    expect(released!.leaseOwner).toBeNull();

    const peer = ledger.takeLease(row0.authKey, "peer-owner", 5_000, nowMs, nowIso);
    expect(peer).not.toBeNull();
    expect(peer!.generation).toBe(2);
    expect(peer!.row.fulfillmentState).toBe("in_progress");

    // Stale unfenced markFailedPermanent (old budget-exhaustion path) — must no-op.
    const clobber = ledger.markFailedPermanent(row0.authKey, nowIso);
    expect(clobber).toBeNull();
    // Stale fence reuse after release must also be rejected.
    expect(
      ledger.markFailedPermanent(row0.authKey, nowIso, {
        owner: "stale-owner",
        generation: 1,
        nowMs,
      }),
    ).toBeNull();
    expect(ledger.getByAuthKey(row0.authKey)!.fulfillmentState).toBe("in_progress");
    expect(ledger.getByAuthKey(row0.authKey)!.leaseGeneration).toBe(2);
    expect(ledger.getByAuthKey(row0.authKey)!.incidents).not.toContain("paid_unfulfilled");

    const committed = ledger.commitFulfillment(
      row0.authKey,
      "peer-owner",
      2,
      nowMs,
      { status: 200, body: { peer: true }, contentType: "application/json" },
      {
        authKey: row0.authKey,
        gatewayTransferId: row0.gatewayTransferId,
        fingerprint: row0.fingerprint,
        fulfilledAt: nowIso,
      },
      nowIso,
    );
    expect(committed).not.toBeNull();
    expect(committed!.fulfillmentState).toBe("fulfilled");
    expect(committed!.storedResponse?.body).toEqual({ peer: true });
    expect(committed!.incidents).not.toContain("paid_unfulfilled");
  });

  it("engine barrier interleaving: stale unfenced permanent cannot overwrite peer 2xx", async () => {
    const reached = deferred();
    const resume = deferred();
    let staleClobberResult: ReturnType<MemoryLedger["markFailedPermanent"]> | undefined;

    const gateway = new FakeGatewayClient({ settleMode: { kind: "success" } });
    const upstream = new FakeUpstreamClient({
      kind: "sequence",
      modes: [{ kind: "retryable" }, { kind: "success", body: { peer: "ok" } }],
    });
    const ledger = new MemoryLedger();
    const engine = new SellerEngine({
      gateway,
      upstream,
      ledger,
      config: {
        network: FIXTURE_NETWORK,
        asset: FIXTURE_ASSET,
        payTo: FIXTURE_PAY_TO,
        verifyingContract: FIXTURE_VERIFYING,
        clock: () => new Date(1_800_000_000_000),
        // First attempt → retryable (not exhausted); hook then simulates the
        // old unfenced permanent follow-up while the peer holds a newer lease.
        fulfillmentRetryBudget: 2,
        fulfillmentRetryBudgetMs: 60_000,
        leaseMs: 30_000,
        leaseWaitMs: 5_000,
      },
      testHooks: {
        afterRetryableRelease: async (row) => {
          reached.resolve();
          await resume.promise;
          staleClobberResult = ledger.markFailedPermanent(
            row.authKey,
            new Date().toISOString(),
          );
        },
      },
    });

    const nonce = fakeNonce(502);
    const payment = makePayment({ nonce });
    const pStale = engine.handle(makeRequest({ payment }));
    await reached.promise;

    upstream.armGate(1);
    const pPeer = engine.handle(makeRequest({ payment }));
    await upstream.whenCalls(2);

    const mid = ledger.getByNonce(nonce)!;
    expect(mid.fulfillmentState).toBe("in_progress");
    expect(mid.leaseGeneration).toBeGreaterThanOrEqual(2);

    resume.resolve();
    await Promise.resolve();
    await Promise.resolve();
    expect(staleClobberResult).toBeNull();

    upstream.releaseGate();
    const [staleRes, peerRes] = await Promise.all([pStale, pPeer]);

    const final = ledger.getByNonce(nonce)!;
    expect(final.fulfillmentState).toBe("fulfilled");
    expect(final.storedResponse).not.toBeNull();
    expect(final.incidents).not.toContain("paid_unfulfilled");
    expect(peerRes.code).toBe("ok");
    // Stale returned retryable 503 before the peer committed; that is fine —
    // the invariant is ledger state + rejected stale permanent + one stored result.
    expect(["ok", "upstream_error"]).toContain(staleRes.code);
    if (staleRes.code === "ok") {
      expect(JSON.stringify(staleRes.body)).toBe(JSON.stringify(peerRes.body));
    }
  });

  it("mirror: genuine budget exhaustion under the holder is failed_permanent atomically", async () => {
    const gateway = new FakeGatewayClient({ settleMode: { kind: "success" } });
    const upstream = new FakeUpstreamClient({ kind: "timeout" });
    const { engine, ledger } = makeSeller({
      gateway,
      upstream,
      fulfillmentRetryBudget: 1,
      fulfillmentRetryBudgetMs: 60_000,
      leaseMs: 5_000,
    });
    const nonce = fakeNonce(503);
    const res = await engine.handle(makeRequest({ payment: makePayment({ nonce }) }));
    expect(res.code).toBe("paid_unfulfilled");
    const row = ledger.getByNonce(nonce)!;
    expect(row.fulfillmentState).toBe("failed_permanent");
    expect(row.incidents).toContain("paid_unfulfilled");
    expect(row.storedResponse).toBeNull();
    expect(row.leaseOwner).toBeNull();
  });

});

describe("resolveRetryableUnderLease atomic CAS", () => {
  it("exhausted under live lease → failed_permanent without an intermediate retryable_failure observe window for peers", async () => {
    const gateway = new FakeGatewayClient({ settleMode: { kind: "success" } });
    const upstream = new FakeUpstreamClient({ kind: "success" });
    const { engine, ledger } = makeSeller({ gateway, upstream });
    const nonce = fakeNonce(504);
    await engine.handle(makeRequest({ payment: makePayment({ nonce }) }));
    const row0 = ledger.getByNonce(nonce)!;
    const row = internalRow(ledger, row0.authKey);
    row.fulfillmentState = "in_progress";
    row.storedResponse = null;
    row.receipt = null;
    row.leaseOwner = "holder";
    row.leaseGeneration = 3;
    row.leaseExpiresAt = Date.now() + 60_000;
    row.fulfillmentAttemptCount = 3;
    row.fulfillmentFirstAttemptAt = new Date().toISOString();
    row.incidents = [];

    const nowMs = Date.now();
    const nowIso = new Date().toISOString();
    const resolved = ledger.resolveRetryableUnderLease(
      row0.authKey,
      "holder",
      3,
      nowMs,
      nowIso,
      { maxAttempts: 3, maxMs: 60_000, nowBusinessMs: nowMs },
    );
    expect(resolved).not.toBeNull();
    expect(resolved!.exhausted).toBe(true);
    expect(resolved!.row.fulfillmentState).toBe("failed_permanent");
    expect(resolved!.row.leaseOwner).toBeNull();
    // Peer cannot take a lease after atomic permanent.
    expect(
      ledger.takeLease(row0.authKey, "peer", 5_000, nowMs, nowIso),
    ).toBeNull();
  });

  it("lost CAS returns null and does not apply permanent under a released fence", async () => {
    const gateway = new FakeGatewayClient({ settleMode: { kind: "success" } });
    const upstream = new FakeUpstreamClient({ kind: "success" });
    const { engine, ledger } = makeSeller({ gateway, upstream });
    const nonce = fakeNonce(505);
    await engine.handle(makeRequest({ payment: makePayment({ nonce }) }));
    const row0 = ledger.getByNonce(nonce)!;
    const row = internalRow(ledger, row0.authKey);
    row.fulfillmentState = "in_progress";
    row.storedResponse = null;
    row.receipt = null;
    row.leaseOwner = "peer";
    row.leaseGeneration = 9;
    row.leaseExpiresAt = Date.now() + 60_000;
    row.fulfillmentAttemptCount = 2;
    row.incidents = [];

    const lost = ledger.resolveRetryableUnderLease(
      row0.authKey,
      "stale",
      8,
      Date.now(),
      new Date().toISOString(),
      { maxAttempts: 1, maxMs: 1, nowBusinessMs: Date.now() },
    );
    expect(lost).toBeNull();
    expect(ledger.getByAuthKey(row0.authKey)!.fulfillmentState).toBe("in_progress");
    expect(ledger.getByAuthKey(row0.authKey)!.leaseOwner).toBe("peer");
  });
});
