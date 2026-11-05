import { describe, expect, it } from "vitest";
import { FakeGatewayClient, makeFakeTransfer } from "../src/gateway/fakeGateway.js";
import { FakeUpstreamClient } from "../src/upstream/fakeUpstream.js";
import {
  SellerEngine,
  isLocallySettled,
  isDirectBatchingEvidence,
} from "../src/seller.js";
import {
  FIXTURE_NETWORK,
  FIXTURE_PAY_TO,
  FIXTURE_PAYER,
  fakeNonce,
  makePayment,
  makeRequest,
  makeSeller,
} from "./helpers.js";

describe("settlement semantics", () => {
  it("only completed is locally settled; received is not batching evidence", () => {
    expect(isLocallySettled(null)).toBe(false);
    expect(isLocallySettled("received")).toBe(false);
    expect(isLocallySettled("batched")).toBe(false);
    expect(isLocallySettled("confirmed")).toBe(false);
    expect(isLocallySettled("completed")).toBe(true);
    expect(isLocallySettled("failed")).toBe(false);

    expect(isDirectBatchingEvidence("received")).toBe(false);
    expect(isDirectBatchingEvidence("batched")).toBe(true);
    expect(isDirectBatchingEvidence("confirmed")).toBe(true);
    expect(isDirectBatchingEvidence("completed")).toBe(false);
  });
});

describe("402 unpaid challenge", () => {
  it("returns payment requirements without ledger or settle", async () => {
    const { engine, gateway, ledger } = makeSeller();
    const res = await engine.handle(makeRequest({ payment: null }));
    expect(res.httpStatus).toBe(402);
    expect(res.code).toBe("payment_required");
    expect(res.paymentRequirements?.network).toBe(FIXTURE_NETWORK);
    expect(res.paymentRequirements?.amount).toBe("10000");
    expect(gateway.settleCalls).toHaveLength(0);
    expect(ledger.getByNonce(fakeNonce(1))).toBeUndefined();
  });
});

describe("ordering: settle before fulfill", () => {
  it("calls FAKE Gateway settle() before FAKE upstream", async () => {
    const order: string[] = [];
    const gateway = new FakeGatewayClient({ settleMode: { kind: "success" } });
    const origSettle = gateway.settle.bind(gateway);
    gateway.settle = async (p, r) => {
      order.push("settle");
      return origSettle(p, r);
    };
    const upstream = new FakeUpstreamClient({ kind: "success" });
    const origFetch = upstream.fetch.bind(upstream);
    upstream.fetch = async (req) => {
      order.push("upstream");
      return origFetch(req);
    };
    const { engine } = makeSeller({ gateway, upstream });
    const payment = makePayment({ nonce: fakeNonce(7), value: "10000" });
    const res = await engine.handle(makeRequest({ payment }));
    expect(res.httpStatus).toBe(200);
    expect(res.code).toBe("ok");
    expect(order).toEqual(["settle", "upstream"]);
    expect(gateway.settleCalls).toHaveLength(1);
    expect(upstream.calls).toHaveLength(1);
  });

  it("never fulfills on definite settle rejection", async () => {
    const gateway = new FakeGatewayClient({
      settleMode: { kind: "reject", errorReason: "insufficient_balance" },
    });
    const upstream = new FakeUpstreamClient({ kind: "success" });
    const { engine, ledger } = makeSeller({ gateway, upstream });
    const payment = makePayment({ nonce: fakeNonce(8) });
    const res = await engine.handle(makeRequest({ payment }));
    expect(res.httpStatus).toBe(402);
    expect(res.code).toBe("payment_rejected");
    expect(res.gatewayErrorReason).toBe("insufficient_balance");
    expect(upstream.calls).toHaveLength(0);
    const row = ledger.getByNonce(fakeNonce(8))!;
    expect(row.paymentState).toBe("rejected");
    expect(row.fulfillmentState).toBe("not_started");
  });
});

describe("automatic void rules", () => {
  it("voids only from submission_pending with attempt counter 0 after validity closes", async () => {
    const { engine, gateway, ledger, setNowMs } = makeSeller();
    // Insert a reserved-but-never-submitted row by exercising validate path then...
    // Use internal ledger insert via a payment that we interrupt: claim never happens if we
    // insert manually.
    const payment = makePayment({
      nonce: fakeNonce(9),
      validAfter: 1_700_000_000,
      validBefore: 1_800_000_000, // seconds
    });
    // Force a reservation without settle by inserting through a custom path:
    // First unpaid? Better: insert via handle that we can't easily interrupt.
    // Use ledger directly with engine's validate by calling handle after manual insert.
    const { buildAuthKey } = await import("../src/authKey.js");
    const { computeFingerprintV1 } = await import("../src/fingerprint.js");
    const fp = computeFingerprintV1({
      method: "GET",
      path: "/v1/balance",
      rawQuery: "address=0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
      bodyByteLength: 0,
      allowedQueryParams: ["address"],
      pathAllowed: true,
    });
    expect(fp.ok).toBe(true);
    if (!fp.ok) return;
    const authKey = buildAuthKey(FIXTURE_NETWORK, FIXTURE_PAYER, fakeNonce(9));
    ledger.insertReservation(
      {
        authKey,
        nonce: fakeNonce(9),
        network: FIXTURE_NETWORK,
        asset: "0x3600000000000000000000000000000000000000",
        payTo: FIXTURE_PAY_TO.toLowerCase(),
        price: "10000",
        payer: FIXTURE_PAYER.toLowerCase(),
        validAfter: 1_700_000_000,
        validBefore: 1_800_000_000,
        fingerprint: fp.fingerprint,
      },
      payment,
      new Date(1_750_000_000_000).toISOString(),
    );
    expect(ledger.getByAuthKey(authKey)!.attemptCounter).toBe(0);
    expect(ledger.getByAuthKey(authKey)!.paymentState).toBe("submission_pending");

    // Advance past validBefore
    setNowMs(1_800_000_001_000);
    const res = await engine.handle(makeRequest({ payment }));
    expect(res.code).toBe("authorization_void");
    expect(res.httpStatus).toBe(409);
    expect(ledger.getByAuthKey(authKey)!.paymentState).toBe("void");
    expect(gateway.settleCalls).toHaveLength(0);
  });

  it("prohibits submission_uncertain → void even after validity closes", async () => {
    const gateway = new FakeGatewayClient({
      settleMode: { kind: "uncertain", reason: "timeout" },
    });
    const { engine, ledger, setNowMs } = makeSeller({ gateway });
    const payment = makePayment({
      nonce: fakeNonce(10),
      validAfter: 1_700_000_000,
      validBefore: 1_800_000_000,
    });
    const res1 = await engine.handle(makeRequest({ payment }));
    expect(res1.code).toBe("payment_submission_unresolved");
    const row = ledger.getByNonce(fakeNonce(10))!;
    expect(row.paymentState).toBe("submission_uncertain");
    expect(row.attemptCounter).toBeGreaterThan(0);

    setNowMs(1_900_000_000_000); // far past validBefore
    // Empty lookup + closed window → stay uncertain, flag unresolved, never void
    gateway.settleMode = { kind: "success" }; // should not be used for void path
    const res2 = await engine.handle(makeRequest({ payment }));
    expect(res2.code).toBe("payment_submission_unresolved");
    const latest = ledger.getByNonce(fakeNonce(10))!;
    expect(latest.paymentState).toBe("submission_uncertain");
    expect(latest.incidents).toContain("submission_unresolved");
    expect(latest.paymentState).not.toBe("void");
  });
});

describe("uncertain submission and late matching transfer", () => {
  it("treats settle timeout as submission_uncertain with no upstream call", async () => {
    const gateway = new FakeGatewayClient({
      settleMode: { kind: "uncertain", reason: "timeout" },
    });
    const upstream = new FakeUpstreamClient({ kind: "success" });
    const { engine, ledger } = makeSeller({ gateway, upstream });
    const payment = makePayment({ nonce: fakeNonce(11) });
    const res = await engine.handle(makeRequest({ payment }));
    expect(res.code).toBe("payment_submission_unresolved");
    expect(upstream.calls).toHaveLength(0);
    expect(ledger.getByNonce(fakeNonce(11))!.paymentState).toBe("submission_uncertain");
  });

  it("adopts a late matching transfer as accepted (even after validity closed)", async () => {
    const gateway = new FakeGatewayClient({
      settleMode: { kind: "uncertain", reason: "timeout" },
    });
    const upstream = new FakeUpstreamClient({ kind: "success" });
    const { engine, ledger, setNowMs } = makeSeller({ gateway, upstream });
    const nonce = fakeNonce(12);
    const payment = makePayment({
      nonce,
      validAfter: 1_700_000_000,
      validBefore: 1_800_000_000,
    });
    await engine.handle(makeRequest({ payment }));
    expect(ledger.getByNonce(nonce)!.paymentState).toBe("submission_uncertain");

    // Validity closed
    setNowMs(1_900_000_000_000);

    // Late matching synthetic transfer appears
    gateway.seedTransfer(
      makeFakeTransfer({
        id: "fake-late-transfer-001",
        nonce,
        fromAddress: FIXTURE_PAYER,
        toAddress: FIXTURE_PAY_TO,
        amount: "10000",
        sendingNetwork: FIXTURE_NETWORK,
        status: "received",
      }),
    );

    const res = await engine.handle(makeRequest({ payment }));
    expect(res.code).toBe("ok");
    expect(res.httpStatus).toBe(200);
    const row = ledger.getByNonce(nonce)!;
    expect(row.paymentState).toBe("accepted");
    expect(row.fulfillmentState).toBe("fulfilled");
    expect(row.paymentState).not.toBe("void");
    expect(upstream.calls).toHaveLength(1);
  });

  it("treats nonce_already_used as uncertain, not rejected", async () => {
    const gateway = new FakeGatewayClient({
      settleMode: { kind: "nonce_already_used" },
    });
    const upstream = new FakeUpstreamClient({ kind: "success" });
    const { engine, ledger } = makeSeller({ gateway, upstream });
    const payment = makePayment({ nonce: fakeNonce(13) });
    const res = await engine.handle(makeRequest({ payment }));
    expect(res.code).toBe("payment_submission_unresolved");
    expect(ledger.getByNonce(fakeNonce(13))!.paymentState).toBe("submission_uncertain");
    expect(upstream.calls).toHaveLength(0);
  });
});

describe("binding mismatch", () => {
  it("rejects same nonce with different fingerprint (409)", async () => {
    const { engine, gateway, upstream } = makeSeller();
    const nonce = fakeNonce(14);
    const payment = makePayment({ nonce });
    const res1 = await engine.handle(
      makeRequest({
        payment,
        rawQuery: "address=0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
      }),
    );
    expect(res1.code).toBe("ok");

    const res2 = await engine.handle(
      makeRequest({
        payment,
        // different address → different fingerprint
        rawQuery: "address=0xd8da6bf26964af9d7eed9e03e53415d37aa96045",
      }),
    );
    expect(res2.httpStatus).toBe(409);
    expect(res2.code).toBe("authorization_binding_mismatch");
    expect(gateway.settleCalls).toHaveLength(1);
    expect(upstream.calls).toHaveLength(1);
  });

  it("rejects same nonce with different validBefore", async () => {
    const { engine, gateway, upstream } = makeSeller();
    const nonce = fakeNonce(15);
    const payment = makePayment({ nonce, validBefore: 1_900_000_000 });
    await engine.handle(makeRequest({ payment }));
    const payment2 = makePayment({ nonce, validBefore: 1_900_000_001 });
    const res = await engine.handle(makeRequest({ payment: payment2 }));
    expect(res.code).toBe("authorization_binding_mismatch");
    expect(res.body).toMatchObject({ field: "validBefore" });
    expect(gateway.settleCalls).toHaveLength(1);
    expect(upstream.calls).toHaveLength(1);
  });

  it("rejects same nonce with different validAfter", async () => {
    const { engine } = makeSeller();
    const nonce = fakeNonce(16);
    await engine.handle(makeRequest({ payment: makePayment({ nonce, validAfter: 100 }) }));
    const res = await engine.handle(
      makeRequest({ payment: makePayment({ nonce, validAfter: 101 }) }),
    );
    expect(res.code).toBe("authorization_binding_mismatch");
    expect(res.body).toMatchObject({ field: "validAfter" });
  });
});

describe("fulfilled retry and gateway observation", () => {
  it("same-binding retry returns stored response with no second settle or upstream", async () => {
    const { engine, gateway, upstream } = makeSeller();
    const payment = makePayment({ nonce: fakeNonce(17) });
    const res1 = await engine.handle(makeRequest({ payment }));
    expect(res1.code).toBe("ok");
    const res2 = await engine.handle(makeRequest({ payment }));
    expect(res2.code).toBe("ok");
    expect(gateway.settleCalls).toHaveLength(1);
    expect(upstream.calls).toHaveLength(1);
  });

  it("observeTransfer copies status; completed is settled; received is not batching", async () => {
    const gateway = new FakeGatewayClient({ settleMode: { kind: "success" } });
    const { engine, ledger } = makeSeller({ gateway });
    const payment = makePayment({ nonce: fakeNonce(18) });
    const res = await engine.handle(makeRequest({ payment }));
    expect(res.code).toBe("ok");
    const row = ledger.getByNonce(fakeNonce(18))!;
    const id = row.gatewayTransferId!;

    // FAKE status progression
    gateway.seedTransfer(
      makeFakeTransfer({
        id,
        nonce: fakeNonce(18),
        fromAddress: FIXTURE_PAYER,
        toAddress: FIXTURE_PAY_TO,
        amount: "10000",
        sendingNetwork: FIXTURE_NETWORK,
        status: "received",
      }),
    );
    let observed = await engine.observeTransfer(row.authKey);
    expect(observed!.gatewayStatus).toBe("received");
    expect(SellerEngine.isDirectBatchingEvidence(observed!.gatewayStatus)).toBe(false);
    expect(SellerEngine.isLocallySettled(observed!.gatewayStatus)).toBe(false);

    gateway.seedTransfer(
      makeFakeTransfer({
        id,
        nonce: fakeNonce(18),
        fromAddress: FIXTURE_PAYER,
        toAddress: FIXTURE_PAY_TO,
        amount: "10000",
        sendingNetwork: FIXTURE_NETWORK,
        status: "batched",
      }),
    );
    observed = await engine.observeTransfer(row.authKey);
    expect(SellerEngine.isDirectBatchingEvidence(observed!.gatewayStatus)).toBe(true);
    expect(SellerEngine.isLocallySettled(observed!.gatewayStatus)).toBe(false);

    gateway.seedTransfer(
      makeFakeTransfer({
        id,
        nonce: fakeNonce(18),
        fromAddress: FIXTURE_PAYER,
        toAddress: FIXTURE_PAY_TO,
        amount: "10000",
        sendingNetwork: FIXTURE_NETWORK,
        status: "completed",
      }),
    );
    observed = await engine.observeTransfer(row.authKey);
    expect(SellerEngine.isLocallySettled(observed!.gatewayStatus)).toBe(true);
  });
});

describe("failed transfer vs fulfillment", () => {
  it("failed before fulfillment → payment_failed, no upstream", async () => {
    const gateway = new FakeGatewayClient({
      settleMode: { kind: "success", transferId: "fake-transfer-fail-before" },
    });
    // Immediately mark failed before fulfill by seeding failed status
    gateway.seedTransfer(
      makeFakeTransfer({
        id: "fake-transfer-fail-before",
        nonce: fakeNonce(19),
        fromAddress: FIXTURE_PAYER,
        toAddress: FIXTURE_PAY_TO,
        amount: "10000",
        sendingNetwork: FIXTURE_NETWORK,
        status: "failed",
      }),
    );
    // Override settle to return success with that id but keep failed transfer
    gateway.settle = async (payload, requirements) => {
      gateway.settleCalls.push({ payload, requirements });
      return {
        success: true,
        payer: FIXTURE_PAYER,
        transaction: "fake-transfer-fail-before",
      };
    };
    const upstream = new FakeUpstreamClient({ kind: "success" });
    const { engine } = makeSeller({ gateway, upstream });
    const res = await engine.handle(makeRequest({ payment: makePayment({ nonce: fakeNonce(19) }) }));
    expect(res.code).toBe("payment_failed");
    expect(res.httpStatus).toBe(402);
    expect(upstream.calls).toHaveLength(0);
  });

  it("failed after fulfillment → keep response + fulfilled_but_unpaid", async () => {
    const gateway = new FakeGatewayClient({ settleMode: { kind: "success" } });
    const { engine, ledger } = makeSeller({ gateway });
    const nonce = fakeNonce(20);
    const res1 = await engine.handle(makeRequest({ payment: makePayment({ nonce }) }));
    expect(res1.code).toBe("ok");
    const row = ledger.getByNonce(nonce)!;
    const id = row.gatewayTransferId!;

    gateway.seedTransfer(
      makeFakeTransfer({
        id,
        nonce,
        fromAddress: FIXTURE_PAYER,
        toAddress: FIXTURE_PAY_TO,
        amount: "10000",
        sendingNetwork: FIXTURE_NETWORK,
        status: "failed",
      }),
    );
    const observed = await engine.observeTransfer(row.authKey);
    expect(observed!.incidents).toContain("fulfilled_but_unpaid");
    expect(observed!.fulfillmentState).toBe("fulfilled");
    expect(observed!.storedResponse).not.toBeNull();

    const res2 = await engine.handle(makeRequest({ payment: makePayment({ nonce }) }));
    expect(res2.code).toBe("ok");
    expect(res2.body).toMatchObject({ incident: "fulfilled_but_unpaid" });
  });
});

describe("permanent upstream failure after acceptance", () => {
  it("marks failed_permanent + paid_unfulfilled", async () => {
    const gateway = new FakeGatewayClient({ settleMode: { kind: "success" } });
    const upstream = new FakeUpstreamClient({
      kind: "permanent",
      status: 400,
      body: { error: "FAKE_permanent" },
    });
    const { engine, ledger } = makeSeller({ gateway, upstream });
    const nonce = fakeNonce(21);
    const res = await engine.handle(makeRequest({ payment: makePayment({ nonce }) }));
    expect(res.httpStatus).toBe(502);
    expect(res.code).toBe("paid_unfulfilled");
    const row = ledger.getByNonce(nonce)!;
    expect(row.paymentState).toBe("accepted");
    expect(row.fulfillmentState).toBe("failed_permanent");
    expect(row.incidents).toContain("paid_unfulfilled");
    expect(gateway.settleCalls).toHaveLength(1);
  });

  it("exhausts retryable budget then paid_unfulfilled", async () => {
    const gateway = new FakeGatewayClient({ settleMode: { kind: "success" } });
    const upstream = new FakeUpstreamClient({ kind: "timeout" });
    const { engine, ledger } = makeSeller({ gateway, upstream });
    const nonce = fakeNonce(22);
    const payment = makePayment({ nonce });
    const r1 = await engine.handle(makeRequest({ payment }));
    expect(r1.code).toBe("upstream_error");
    const r2 = await engine.handle(makeRequest({ payment }));
    expect(r2.code).toBe("paid_unfulfilled");
    const row = ledger.getByNonce(nonce)!;
    expect(row.fulfillmentState).toBe("failed_permanent");
    expect(row.incidents).toContain("paid_unfulfilled");
    expect(row.paymentState).toBe("accepted"); // never voided
  });
});

describe("lease fencing", () => {
  it("stale worker cannot overwrite a stored fulfillment", async () => {
    const { engine, ledger } = makeSeller();
    const nonce = fakeNonce(23);
    const payment = makePayment({ nonce });
    await engine.handle(makeRequest({ payment }));
    const row = ledger.getByNonce(nonce)!;
    expect(row.fulfillmentState).toBe("fulfilled");

    const overwritten = ledger.commitFulfillment(
      row.authKey,
      "stale-worker",
      { status: 200, body: { evil: true }, contentType: "application/json" },
      {
        authKey: row.authKey,
        gatewayTransferId: row.gatewayTransferId,
        fingerprint: row.fingerprint,
        fulfilledAt: "1970-01-01T00:00:00.000Z",
      },
      new Date().toISOString(),
    );
    expect(overwritten).toBeNull();
    expect(ledger.getByAuthKey(row.authKey)!.storedResponse?.body).not.toMatchObject({
      evil: true,
    });
  });
});

describe("one settle for concurrent first arrivals (serial lock)", () => {
  it("second same-binding call does not settle again after acceptance", async () => {
    const { engine, gateway, upstream } = makeSeller();
    const payment = makePayment({ nonce: fakeNonce(24) });
    const results = await Promise.all([
      engine.handle(makeRequest({ payment })),
      engine.handle(makeRequest({ payment })),
    ]);
    expect(results.every((r) => r.code === "ok" || r.code === "upstream_error")).toBe(true);
    // With mutex, both complete; settle exactly once
    expect(gateway.settleCalls).toHaveLength(1);
    // Upstream may be 1 (second served from store) — allow 1
    expect(upstream.calls.length).toBeLessThanOrEqual(2);
    expect(upstream.calls.length).toBeGreaterThanOrEqual(1);
  });
});
