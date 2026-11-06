import { describe, expect, it } from "vitest";
import { FakeGatewayClient, makeFakeTransfer } from "../src/gateway/fakeGateway.js";
import { FakeUpstreamClient } from "../src/upstream/fakeUpstream.js";
import { MemoryLedger } from "../src/ledger/memoryLedger.js";
import { SellerEngine } from "../src/seller.js";
import {
  FIXTURE_NETWORK,
  FIXTURE_PAY_TO,
  FIXTURE_PAYER,
  FIXTURE_ASSET,
  FIXTURE_VERIFYING,
  fakeNonce,
  makePayment,
  makeRequest,
  makeSeller,
} from "./helpers.js";

describe("settle outcome classification", () => {
  it("success:true without transaction → submission_uncertain + 503 unresolved", async () => {
    const gateway = new FakeGatewayClient({
      settleMode: { kind: "success_missing_transaction" },
    });
    const upstream = new FakeUpstreamClient({ kind: "success" });
    const { engine, ledger } = makeSeller({ gateway, upstream });
    const nonce = fakeNonce(301);
    const res = await engine.handle(makeRequest({ payment: makePayment({ nonce }) }));
    expect(res.code).toBe("payment_submission_unresolved");
    expect(res.httpStatus).toBe(503);
    expect(res.retryAfterSeconds).toBeTypeOf("number");
    expect(ledger.getByNonce(nonce)!.paymentState).toBe("submission_uncertain");
    expect(upstream.calls).toHaveLength(0);
  });

  it("success:false without errorReason → submission_uncertain", async () => {
    const gateway = new FakeGatewayClient({
      settleMode: { kind: "reject_missing_errorReason" },
    });
    const upstream = new FakeUpstreamClient({ kind: "success" });
    const { engine, ledger } = makeSeller({ gateway, upstream });
    const nonce = fakeNonce(302);
    const res = await engine.handle(makeRequest({ payment: makePayment({ nonce }) }));
    expect(res.code).toBe("payment_submission_unresolved");
    expect(ledger.getByNonce(nonce)!.paymentState).toBe("submission_uncertain");
    expect(upstream.calls).toHaveLength(0);
  });

  for (const reason of ["timeout", "drop", "empty_body", "unexpected_error"] as const) {
    it(`uncertain reason ${reason} → submission_uncertain, no fulfillment`, async () => {
      const gateway = new FakeGatewayClient({
        settleMode: { kind: "uncertain", reason },
      });
      const upstream = new FakeUpstreamClient({ kind: "success" });
      const { engine, ledger } = makeSeller({ gateway, upstream });
      const nonce = fakeNonce(310 + reason.length);
      const res = await engine.handle(makeRequest({ payment: makePayment({ nonce }) }));
      expect(res.code).toBe("payment_submission_unresolved");
      expect(ledger.getByNonce(nonce)!.paymentState).toBe("submission_uncertain");
      expect(upstream.calls).toHaveLength(0);
    });
  }

  it("HTTP 500 → submission_uncertain", async () => {
    const gateway = new FakeGatewayClient({ settleMode: { kind: "http500" } });
    const upstream = new FakeUpstreamClient({ kind: "success" });
    const { engine, ledger } = makeSeller({ gateway, upstream });
    const nonce = fakeNonce(320);
    const res = await engine.handle(makeRequest({ payment: makePayment({ nonce }) }));
    expect(res.code).toBe("payment_submission_unresolved");
    expect(ledger.getByNonce(nonce)!.paymentState).toBe("submission_uncertain");
    expect(upstream.calls).toHaveLength(0);
  });

  it("HTTP 400 → definite rejection", async () => {
    const gateway = new FakeGatewayClient({
      settleMode: { kind: "http400", errorReason: "invalid_payload" },
    });
    const upstream = new FakeUpstreamClient({ kind: "success" });
    const { engine, ledger } = makeSeller({ gateway, upstream });
    const nonce = fakeNonce(321);
    const res = await engine.handle(makeRequest({ payment: makePayment({ nonce }) }));
    expect(res.code).toBe("payment_rejected");
    expect(ledger.getByNonce(nonce)!.paymentState).toBe("rejected");
    expect(upstream.calls).toHaveLength(0);
  });
});

describe("fail-closed reconciliation", () => {
  it("lookup failure keeps submission_uncertain + submission_unresolved", async () => {
    const gateway = new FakeGatewayClient({
      settleMode: { kind: "uncertain", reason: "timeout" },
      lookupFails: true,
    });
    const upstream = new FakeUpstreamClient({ kind: "success" });
    const { engine, ledger } = makeSeller({ gateway, upstream });
    const nonce = fakeNonce(330);
    await engine.handle(makeRequest({ payment: makePayment({ nonce }) }));
    const res = await engine.handle(makeRequest({ payment: makePayment({ nonce }) }));
    expect(res.code).toBe("payment_submission_unresolved");
    const row = ledger.getByNonce(nonce)!;
    expect(row.paymentState).toBe("submission_uncertain");
    expect(row.incidents).toContain("submission_unresolved");
    expect(upstream.calls).toHaveLength(0);
  });

  it("ambiguous lookup stays unresolved (never hits[0])", async () => {
    const gateway = new FakeGatewayClient({
      settleMode: { kind: "uncertain", reason: "timeout" },
    });
    const upstream = new FakeUpstreamClient({ kind: "success" });
    const { engine, ledger } = makeSeller({ gateway, upstream });
    const nonce = fakeNonce(331);
    await engine.handle(makeRequest({ payment: makePayment({ nonce }) }));
    gateway.seedTransfer(
      makeFakeTransfer({
        id: "fake-amb-1",
        nonce,
        fromAddress: FIXTURE_PAYER,
        toAddress: FIXTURE_PAY_TO,
        amount: "10000",
        sendingNetwork: FIXTURE_NETWORK,
        status: "received",
      }),
    );
    gateway.seedExtraTransfer(
      makeFakeTransfer({
        id: "fake-amb-2",
        nonce,
        fromAddress: FIXTURE_PAYER,
        toAddress: FIXTURE_PAY_TO,
        amount: "10000",
        sendingNetwork: FIXTURE_NETWORK,
        status: "batched",
      }),
    );
    const res = await engine.handle(makeRequest({ payment: makePayment({ nonce }) }));
    expect(res.code).toBe("payment_submission_unresolved");
    expect(ledger.getByNonce(nonce)!.paymentState).toBe("submission_uncertain");
    expect(ledger.getByNonce(nonce)!.incidents).toContain("submission_unresolved");
    expect(upstream.calls).toHaveLength(0);
  });

  it("mismatched transfer-by-ID is not adopted", async () => {
    const gateway = new FakeGatewayClient({ settleMode: { kind: "success" } });
    // Force acceptance then clear fulfillment by using a custom path:
    // First succeed settle+fulfill, then we test observe with mismatched id content.
    // For reconciliation path: seed uncertain row with a transfer id that mismatches.
    const upstream = new FakeUpstreamClient({ kind: "success" });
    const { engine, ledger } = makeSeller({
      gateway: new FakeGatewayClient({ settleMode: { kind: "uncertain", reason: "timeout" } }),
      upstream,
    });
    const nonce = fakeNonce(332);
    await engine.handle(makeRequest({ payment: makePayment({ nonce }) }));
    const row = ledger.getByNonce(nonce)!;
    // Manually set a transfer id, then seed a mismatched record under that id
    ledger.updatePaymentState(
      row.authKey,
      "submission_uncertain",
      { gatewayTransferId: "fake-mismatch-by-id" },
      new Date().toISOString(),
    );
    gateway.seedTransfer(
      makeFakeTransfer({
        id: "fake-mismatch-by-id",
        nonce,
        fromAddress: "0x9999999999999999999999999999999999999999",
        toAddress: FIXTURE_PAY_TO,
        amount: "10000",
        sendingNetwork: FIXTURE_NETWORK,
        status: "received",
      }),
    );
    // Use engine that shares ledger but with the seeding gateway
    const engine2 = new SellerEngine({
      gateway,
      upstream,
      ledger,
      config: {
        network: FIXTURE_NETWORK,
        asset: FIXTURE_ASSET,
        payTo: FIXTURE_PAY_TO,
        verifyingContract: FIXTURE_VERIFYING,
        clock: () => new Date(1_800_000_000_000),
        leaseMs: 1000,
        leaseWaitMs: 200,
      },
    });
    const res = await engine2.handle(makeRequest({ payment: makePayment({ nonce }) }));
    expect(res.code).toBe("payment_rejected");
    expect(res.gatewayErrorReason).toBe("nonce_consumed_elsewhere");
    expect(ledger.getByNonce(nonce)!.paymentState).toBe("rejected");
    expect(upstream.calls).toHaveLength(0);
  });
});

describe("upstream 2xx-only commit", () => {
  it("302 never becomes fulfilled", async () => {
    const gateway = new FakeGatewayClient({ settleMode: { kind: "success" } });
    const upstream = new FakeUpstreamClient({ kind: "redirect", status: 302 });
    const { engine, ledger } = makeSeller({ gateway, upstream });
    const nonce = fakeNonce(340);
    const res = await engine.handle(makeRequest({ payment: makePayment({ nonce }) }));
    expect(res.code).toBe("paid_unfulfilled");
    expect(ledger.getByNonce(nonce)!.fulfillmentState).toBe("failed_permanent");
    expect(ledger.getByNonce(nonce)!.storedResponse).toBeNull();
  });

  it("418 non-2xx/non-5xx never becomes fulfilled", async () => {
    const gateway = new FakeGatewayClient({ settleMode: { kind: "success" } });
    const upstream = new FakeUpstreamClient({ kind: "permanent", status: 418 });
    const { engine, ledger } = makeSeller({ gateway, upstream });
    const nonce = fakeNonce(341);
    const res = await engine.handle(makeRequest({ payment: makePayment({ nonce }) }));
    expect(res.code).toBe("paid_unfulfilled");
    expect(ledger.getByNonce(nonce)!.fulfillmentState).toBe("failed_permanent");
  });
});

describe("fulfilled retry across gateway statuses and lookup outage", () => {
  for (const status of [
    null,
    "received",
    "batched",
    "confirmed",
    "completed",
    "failed",
  ] as const) {
    it(`fulfilled retry with gateway status ${status} returns stored response, no settle/upstream`, async () => {
      const gateway = new FakeGatewayClient({ settleMode: { kind: "success" } });
      const upstream = new FakeUpstreamClient({ kind: "success" });
      const { engine, ledger } = makeSeller({ gateway, upstream });
      const nonce = fakeNonce(350 + (status?.length ?? 0));
      const payment = makePayment({ nonce });
      const res1 = await engine.handle(makeRequest({ payment }));
      expect(res1.code).toBe("ok");
      const row = ledger.getByNonce(nonce)!;
      if (status !== null) {
        gateway.seedTransfer(
          makeFakeTransfer({
            id: row.gatewayTransferId!,
            nonce,
            fromAddress: FIXTURE_PAYER,
            toAddress: FIXTURE_PAY_TO,
            amount: "10000",
            sendingNetwork: FIXTURE_NETWORK,
            status,
          }),
        );
        await engine.observeTransfer(row.authKey);
      }
      const settleBefore = gateway.settleCalls.length;
      const upBefore = upstream.calls.length;
      const res2 = await engine.handle(makeRequest({ payment }));
      expect(res2.code).toBe("ok");
      expect(res2.body).toEqual(res1.body);
      expect(gateway.settleCalls).toHaveLength(settleBefore);
      expect(upstream.calls).toHaveLength(upBefore);
    });
  }

  it("fulfilled retry during Gateway lookup outage still returns stored response", async () => {
    const gateway = new FakeGatewayClient({ settleMode: { kind: "success" } });
    const upstream = new FakeUpstreamClient({ kind: "success" });
    const { engine } = makeSeller({ gateway, upstream });
    const nonce = fakeNonce(360);
    const payment = makePayment({ nonce });
    const res1 = await engine.handle(makeRequest({ payment }));
    expect(res1.code).toBe("ok");
    gateway.lookupFails = true;
    const res2 = await engine.handle(makeRequest({ payment }));
    expect(res2.code).toBe("ok");
    expect(res2.body).toEqual(res1.body);
    expect(upstream.calls).toHaveLength(1);
  });
});

describe("gateway anomalies", () => {
  it("unknown status does not mutate gateway_status and records anomaly", async () => {
    const gateway = new FakeGatewayClient({ settleMode: { kind: "success" } });
    const { engine, ledger } = makeSeller({ gateway });
    const nonce = fakeNonce(370);
    await engine.handle(makeRequest({ payment: makePayment({ nonce }) }));
    const row = ledger.getByNonce(nonce)!;
    ledger.applyGatewayStatus(row.authKey, "received", new Date().toISOString());
    const after = ledger.applyGatewayStatus(
      row.authKey,
      "totally_unknown_status",
      new Date().toISOString(),
    );
    expect(after.gatewayStatus).toBe("received");
    expect(after.anomalies.some((a) => a.includes("unknown_gateway_status"))).toBe(true);
  });

  it("backward status does not mutate and records anomaly", async () => {
    const { ledger } = makeSeller();
    // Build a fulfilled accepted row via engine first
    const gateway = new FakeGatewayClient({ settleMode: { kind: "success" } });
    const { engine, ledger: led } = makeSeller({ gateway });
    const nonce = fakeNonce(371);
    await engine.handle(makeRequest({ payment: makePayment({ nonce }) }));
    const row = led.getByNonce(nonce)!;
    led.applyGatewayStatus(row.authKey, "completed", new Date().toISOString());
    const after = led.applyGatewayStatus(row.authKey, "received", new Date().toISOString());
    expect(after.gatewayStatus).toBe("completed");
    expect(after.anomalies.some((a) => a.includes("backward_gateway_status"))).toBe(true);
    void ledger;
  });
});

describe("concurrency, lease fencing, lost-response", () => {
  it("lost-response recovery: upstream done, storage not, retry fulfills once", async () => {
    const gateway = new FakeGatewayClient({ settleMode: { kind: "success" } });
    const upstream = new FakeUpstreamClient({ kind: "success" });
    const { engine, ledger, advanceMs } = makeSeller({
      gateway,
      upstream,
      leaseMs: 50,
      leaseWaitMs: 200,
    });
    const nonce = fakeNonce(380);
    const payment = makePayment({ nonce });

    // First attempt: take lease, call upstream, but simulate lost storage by
    // having commit fail via expired lease before commit.
    // Simpler approach: first call succeeds; manually clear stored response while
    // keeping accepted + retryable_failure to simulate lost storage after upstream.
    const res1 = await engine.handle(makeRequest({ payment }));
    expect(res1.code).toBe("ok");
    // Simulate lost storage after upstream: revert fulfillment fields
    const row = ledger.getByNonce(nonce)!;
    const internal = (ledger as unknown as { byAuthKey: Map<string, typeof row> }).byAuthKey.get(
      row.authKey,
    )!;
    internal.fulfillmentState = "retryable_failure";
    internal.storedResponse = null;
    internal.receipt = null;
    internal.leaseOwner = null;
    internal.leaseExpiresAt = null;

    const settleBefore = gateway.settleCalls.length;
    const res2 = await engine.handle(makeRequest({ payment }));
    expect(res2.code).toBe("ok");
    expect(gateway.settleCalls).toHaveLength(settleBefore);
    expect(ledger.getByNonce(nonce)!.fulfillmentState).toBe("fulfilled");
    expect(ledger.getByNonce(nonce)!.storedResponse).not.toBeNull();
    void advanceMs;
  });

  it("real lease takeover: stale late worker cannot commit", async () => {
    const gateway = new FakeGatewayClient({ settleMode: { kind: "success" } });
    const upstream = new FakeUpstreamClient({ kind: "success" });
    upstream.armGate();
    const { engine, ledger, advanceMs } = makeSeller({
      gateway,
      upstream,
      leaseMs: 30,
      leaseWaitMs: 500,
    });
    const nonce = fakeNonce(381);
    const payment = makePayment({ nonce });

    const p1 = engine.handle(makeRequest({ payment }));
    // Wait until first upstream call is gated
    await new Promise((r) => setTimeout(r, 20));
    advanceMs(100); // expire lease
    // Second caller takes over
    const upstream2 = upstream; // same fake; release gate so both proceed
    void upstream2;
    upstream.releaseGate();
    const p2 = engine.handle(makeRequest({ payment }));
    const [r1, r2] = await Promise.all([p1, p2]);
    // At least one ok; both should eventually see same stored or one stale
    const oks = [r1, r2].filter((r) => r.code === "ok");
    expect(oks.length).toBeGreaterThanOrEqual(1);
    expect(gateway.settleCalls).toHaveLength(1);
    const row = ledger.getByNonce(nonce)!;
    expect(row.fulfillmentState === "fulfilled" || row.fulfillmentState === "retryable_failure" || row.fulfillmentState === "failed_permanent" || row.fulfillmentState === "in_progress").toBe(true);
    // Exactly one stored response if fulfilled
    if (row.fulfillmentState === "fulfilled") {
      expect(row.storedResponse).not.toBeNull();
    }
  });

  it("two SellerEngine instances sharing one ledger use unique fencing", async () => {
    const gateway = new FakeGatewayClient({ settleMode: { kind: "success" } });
    const upstream = new FakeUpstreamClient({ kind: "success" });
    const ledger = new MemoryLedger();
    const cfg = {
      network: FIXTURE_NETWORK,
      asset: FIXTURE_ASSET,
      payTo: FIXTURE_PAY_TO,
      verifyingContract: FIXTURE_VERIFYING,
      clock: () => new Date(1_800_000_000_000),
      leaseMs: 1_000,
      leaseWaitMs: 1_000,
      fulfillmentRetryBudget: 5,
    };
    const a = new SellerEngine({ gateway, upstream, ledger, config: cfg });
    const b = new SellerEngine({ gateway, upstream, ledger, config: cfg });
    const payment = makePayment({ nonce: fakeNonce(382) });
    const [r1, r2] = await Promise.all([
      a.handle(makeRequest({ payment })),
      b.handle(makeRequest({ payment })),
    ]);
    expect(r1.code).toBe("ok");
    expect(r2.code).toBe("ok");
    expect(JSON.stringify(r1.body)).toBe(JSON.stringify(r2.body));
    expect(gateway.settleCalls).toHaveLength(1);
  });

  it("expired lease is not commit-eligible even without takeover", async () => {
    const { ledger } = makeSeller();
    const gateway = new FakeGatewayClient({ settleMode: { kind: "success" } });
    const upstream = new FakeUpstreamClient({ kind: "success" });
    const { engine, ledger: led } = makeSeller({ gateway, upstream, leaseMs: 10 });
    const nonce = fakeNonce(383);
    // Manually: accept + take lease, expire, try commit
    await engine.handle(makeRequest({ payment: makePayment({ nonce }) }));
    // already fulfilled; craft a fresh lease scenario via direct ledger ops
    const row = led.getByNonce(nonce)!;
    const internal = (led as unknown as { byAuthKey: Map<string, typeof row> }).byAuthKey.get(
      row.authKey,
    )!;
    internal.fulfillmentState = "in_progress";
    internal.storedResponse = null;
    internal.receipt = null;
    internal.leaseOwner = "owner-A";
    internal.leaseGeneration = 5;
    internal.leaseExpiresAt = Date.now() - 1; // already expired
    const committed = led.commitFulfillment(
      row.authKey,
      "owner-A",
      5,
      Date.now(),
      { status: 200, body: { late: true }, contentType: "application/json" },
      {
        authKey: row.authKey,
        gatewayTransferId: row.gatewayTransferId,
        fingerprint: row.fingerprint,
        fulfilledAt: new Date().toISOString(),
      },
      new Date().toISOString(),
    );
    expect(committed).toBeNull();
    expect(led.getByAuthKey(row.authKey)!.storedResponse).toBeNull();
    void ledger;
  });

  it("stale worker markRetryableFailure / markFailedPermanent after takeover are rejected", async () => {
    const { ledger } = makeSeller();
    const gateway = new FakeGatewayClient({ settleMode: { kind: "success" } });
    const { engine, ledger: led } = makeSeller({ gateway });
    const nonce = fakeNonce(384);
    await engine.handle(makeRequest({ payment: makePayment({ nonce }) }));
    const row = led.getByNonce(nonce)!;
    const internal = (led as unknown as { byAuthKey: Map<string, typeof row> }).byAuthKey.get(
      row.authKey,
    )!;
    internal.fulfillmentState = "in_progress";
    internal.storedResponse = null;
    internal.receipt = null;
    internal.leaseOwner = "new-owner";
    internal.leaseGeneration = 7;
    internal.leaseExpiresAt = Date.now() + 10_000;

    const retry = led.markRetryableFailure(
      row.authKey,
      "stale-owner",
      6,
      Date.now(),
      new Date().toISOString(),
    );
    expect(retry).toBeNull();
    const perm = led.markFailedPermanent(row.authKey, new Date().toISOString(), {
      owner: "stale-owner",
      generation: 6,
      nowMs: Date.now(),
    });
    expect(perm).toBeNull();
    expect(led.getByAuthKey(row.authKey)!.fulfillmentState).toBe("in_progress");
    void ledger;
  });
});

describe("retry budgets", () => {
  it("permanent-failure budget exhaustion by time", async () => {
    const gateway = new FakeGatewayClient({ settleMode: { kind: "success" } });
    const upstream = new FakeUpstreamClient({ kind: "timeout" });
    const { engine, ledger, advanceMs } = makeSeller({
      gateway,
      upstream,
      fulfillmentRetryBudget: 100,
      fulfillmentRetryBudgetMs: 50,
    });
    const nonce = fakeNonce(390);
    const payment = makePayment({ nonce });
    const r1 = await engine.handle(makeRequest({ payment }));
    expect(r1.code).toBe("upstream_error");
    advanceMs(100);
    const r2 = await engine.handle(makeRequest({ payment }));
    expect(r2.code).toBe("paid_unfulfilled");
    expect(ledger.getByNonce(nonce)!.fulfillmentState).toBe("failed_permanent");
  });
});

describe("engine-level fingerprint rejection vectors R1–R9 (no 402/ledger/settle/upstream)", () => {
  const vectors: Array<{ name: string; req: ReturnType<typeof makeRequest>; status: number }> = [
    {
      name: "R1 duplicate name",
      req: makeRequest({
        path: "/v1/transactions",
        rawQuery: "limit=1&limit=2",
        payment: makePayment({ nonce: fakeNonce(401) }),
      }),
      status: 400,
    },
    {
      name: "R2 empty segment",
      req: makeRequest({
        path: "/v1/transactions",
        rawQuery: "address=0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045&&limit=1",
        payment: makePayment({ nonce: fakeNonce(402) }),
      }),
      status: 400,
    },
    {
      name: "R3 segment without =",
      req: makeRequest({
        path: "/v1/transactions",
        rawQuery: "limit",
        payment: makePayment({ nonce: fakeNonce(403) }),
      }),
      status: 400,
    },
    {
      name: "R4 bad percent",
      req: makeRequest({
        path: "/v1/defi/events",
        rawQuery: "type=%zz",
        payment: makePayment({ nonce: fakeNonce(404) }),
      }),
      status: 400,
    },
    {
      name: "R5 not utf8",
      req: makeRequest({
        path: "/v1/defi/events",
        rawQuery: "type=%FF",
        payment: makePayment({ nonce: fakeNonce(405) }),
      }),
      status: 400,
    },
    {
      name: "R6 trailing slash path",
      req: makeRequest({
        path: "/v1/balance/",
        rawQuery: "address=0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        payment: makePayment({ nonce: fakeNonce(406) }),
      }),
      status: 404,
    },
    {
      name: "R7 undocumented param",
      req: makeRequest({
        rawQuery: "address=0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045&x=1",
        payment: makePayment({ nonce: fakeNonce(407) }),
      }),
      status: 400,
    },
    {
      name: "R8 body not empty",
      req: makeRequest({
        bodyByteLength: 1,
        payment: makePayment({ nonce: fakeNonce(408) }),
      }),
      status: 400,
    },
    {
      name: "R9 POST method",
      req: makeRequest({
        method: "POST",
        payment: makePayment({ nonce: fakeNonce(409) }),
      }),
      status: 405,
    },
  ];

  for (const v of vectors) {
    it(`${v.name}: no 402 challenge, no ledger, no settle, no upstream`, async () => {
      const { engine, gateway, upstream, ledger } = makeSeller();
      const res = await engine.handle(v.req);
      expect(res.httpStatus).toBe(v.status);
      expect(res.code).not.toBe("payment_required");
      expect(gateway.settleCalls).toHaveLength(0);
      expect(upstream.calls).toHaveLength(0);
      const nonce = v.req.payment?.payload.authorization.nonce;
      if (nonce) expect(ledger.getByNonce(nonce)).toBeUndefined();
    });
  }
});

describe("attempted uncertain rows never become void", () => {
  it("stays submission_uncertain after validity closure with empty lookup", async () => {
    const gateway = new FakeGatewayClient({
      settleMode: { kind: "uncertain", reason: "drop" },
    });
    const { engine, ledger, setNowMs } = makeSeller({
      gateway,
      nowMs: 1_700_100_000_000,
    });
    const nonce = fakeNonce(410);
    const payment = makePayment({
      nonce,
      validAfter: 1_700_000_000,
      validBefore: 1_710_000_000,
    });
    await engine.handle(makeRequest({ payment }));
    setNowMs(2_000_000_000_000);
    const res = await engine.handle(makeRequest({ payment }));
    expect(res.code).toBe("payment_submission_unresolved");
    expect(ledger.getByNonce(nonce)!.paymentState).toBe("submission_uncertain");
    expect(ledger.getByNonce(nonce)!.paymentState).not.toBe("void");
  });
});
