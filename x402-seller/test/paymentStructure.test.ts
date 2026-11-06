import { describe, expect, it } from "vitest";
import {
  FIXTURE_ASSET,
  FIXTURE_NETWORK,
  FIXTURE_PAY_TO,
  FIXTURE_PAYER,
  FIXTURE_VERIFYING,
  fakeNonce,
  makePayment,
  makeRequest,
  makeSeller,
} from "./helpers.js";
import { parseStrictSafeInteger, PaymentValidationError } from "../src/paymentValidation.js";

describe("parseStrictSafeInteger", () => {
  it("accepts non-negative safe integers as number or digit string", () => {
    expect(parseStrictSafeInteger(0, "n")).toBe(0);
    expect(parseStrictSafeInteger(1900000000, "n")).toBe(1900000000);
    expect(parseStrictSafeInteger("0", "n")).toBe(0);
    expect(parseStrictSafeInteger("1900000000", "n")).toBe(1900000000);
  });

  it("rejects junk suffixes, fractions, unsafe ints, malformed strings", () => {
    const bad = [
      "1900000000junk",
      "1.5",
      1.5,
      "-1",
      -1,
      "",
      " 1",
      "01",
      "0x10",
      Number.MAX_SAFE_INTEGER + 1,
      "9007199254740992",
      NaN,
      null,
      undefined,
      {},
    ];
    for (const v of bad) {
      expect(() => parseStrictSafeInteger(v as never, "n")).toThrow(PaymentValidationError);
    }
  });
});

describe("payment structure rejections (new nonce, zero effects)", () => {
  const cases: Array<{ name: string; patch: () => ReturnType<typeof makePayment> | Record<string, unknown> }> = [
    {
      name: "wrong x402Version",
      patch: () => makePayment({ x402Version: 1 as unknown as 2, nonce: fakeNonce(101) }),
    },
    {
      name: "missing accepted",
      patch: () => makePayment({ omitAccepted: true, nonce: fakeNonce(102) }),
    },
    {
      name: "wrong scheme",
      patch: () => makePayment({ scheme: "upto", nonce: fakeNonce(103) }),
    },
    {
      name: "network mismatch",
      patch: () => makePayment({ network: "eip155:1", nonce: fakeNonce(104) }),
    },
    {
      name: "asset mismatch",
      patch: () =>
        makePayment({
          asset: "0x9999999999999999999999999999999999999999",
          nonce: fakeNonce(105),
        }),
    },
    {
      name: "amount mismatch",
      patch: () => makePayment({ value: "99999", nonce: fakeNonce(106) }),
    },
    {
      name: "payTo mismatch",
      patch: () =>
        makePayment({
          to: "0x9999999999999999999999999999999999999999",
          nonce: fakeNonce(107),
        }),
    },
    {
      name: "verifyingContract mismatch",
      patch: () =>
        makePayment({
          verifyingContract: "0x0000000000000000000000000000000000000001",
          nonce: fakeNonce(108),
        }),
    },
    {
      name: "missing signature",
      patch: () => {
        const p = makePayment({ nonce: fakeNonce(109) });
        (p.payload as { signature?: string }).signature = "";
        return p;
      },
    },
    {
      name: "auth.to inconsistent with accepted.payTo",
      patch: () => {
        const p = makePayment({ nonce: fakeNonce(110) });
        p.payload.authorization.to = "0x9999999999999999999999999999999999999999";
        return p;
      },
    },
    {
      name: "auth.value inconsistent with accepted.amount",
      patch: () => {
        const p = makePayment({ nonce: fakeNonce(111) });
        p.payload.authorization.value = "1";
        return p;
      },
    },
    {
      name: "non-32-byte nonce",
      patch: () => makePayment({ nonce: "0xabc", }),
    },
    {
      name: "junk validAfter string",
      patch: () => {
        const p = makePayment({ nonce: fakeNonce(112) });
        (p.payload.authorization as { validAfter: string | number }).validAfter =
          "1900000000junk";
        return p;
      },
    },
    {
      name: "fractional validBefore",
      patch: () => {
        const p = makePayment({ nonce: fakeNonce(113) });
        (p.payload.authorization as { validBefore: string | number }).validBefore = 1.5;
        return p;
      },
    },
    {
      name: "validAfter > validBefore",
      patch: () =>
        makePayment({
          nonce: fakeNonce(114),
          validAfter: 1_900_000_000,
          validBefore: 1_800_000_000,
        }),
    },
    {
      name: "validity window too short (< 3 days ahead)",
      patch: () =>
        makePayment({
          nonce: fakeNonce(115),
          validAfter: 1_700_000_000,
          // nowSec is 1_800_000_000; need validBefore >= now+259200
          validBefore: 1_800_000_100,
        }),
    },
  ];

  for (const c of cases) {
    it(`rejects ${c.name} with no ledger/settle/upstream`, async () => {
      const { engine, gateway, upstream, ledger } = makeSeller();
      const payment = c.patch() as ReturnType<typeof makePayment>;
      const res = await engine.handle(makeRequest({ payment }));
      expect(res.httpStatus).toBeGreaterThanOrEqual(400);
      expect(res.httpStatus).toBeLessThan(500);
      expect(res.code).toBe("bad_request");
      expect(gateway.settleCalls).toHaveLength(0);
      expect(upstream.calls).toHaveLength(0);
      // no ledger row for this nonce (if nonce was parseable)
      const nonce = payment?.payload?.authorization?.nonce;
      if (typeof nonce === "string" && /^0x[0-9a-fA-F]{64}$/.test(nonce)) {
        expect(ledger.getByNonce(nonce)).toBeUndefined();
      }
    });
  }
});

describe("binding mismatch — all eight fields", () => {
  const fields: Array<{
    field: string;
    mutate: (nonce: string) => ReturnType<typeof makePayment>;
  }> = [
    {
      field: "payer",
      mutate: (nonce) =>
        makePayment({
          nonce,
          from: "0x3333333333333333333333333333333333333333",
        }),
    },
    {
      field: "network",
      mutate: (nonce) => makePayment({ nonce, network: "eip155:1" }),
    },
    {
      field: "asset",
      mutate: (nonce) =>
        makePayment({
          nonce,
          asset: "0x4444444444444444444444444444444444444444",
        }),
    },
    {
      field: "payTo",
      mutate: (nonce) =>
        makePayment({
          nonce,
          to: "0x5555555555555555555555555555555555555555",
        }),
    },
    {
      field: "price",
      mutate: (nonce) => makePayment({ nonce, value: "20000" }),
    },
    {
      field: "validAfter",
      mutate: (nonce) => makePayment({ nonce, validAfter: 1_700_000_001 }),
    },
    {
      field: "validBefore",
      mutate: (nonce) => makePayment({ nonce, validBefore: 1_900_000_001 }),
    },
    {
      field: "fingerprint",
      mutate: (nonce) => makePayment({ nonce }),
    },
  ];

  for (const [i, f] of fields.entries()) {
    it(`rejects same nonce with different ${f.field} (409, no state/Gateway/upstream change)`, async () => {
      const { engine, gateway, upstream, ledger } = makeSeller();
      const nonce = fakeNonce(200 + i);
      const first = makePayment({ nonce });
      const res1 = await engine.handle(
        makeRequest({
          payment: first,
          rawQuery: "address=0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        }),
      );
      expect(res1.code).toBe("ok");
      const settleBefore = gateway.settleCalls.length;
      const upstreamBefore = upstream.calls.length;
      const rowBefore = structuredClone(ledger.getByNonce(nonce)!);

      const second =
        f.field === "fingerprint"
          ? makeRequest({
              payment: makePayment({ nonce }),
              rawQuery: "address=0xd8da6bf26964af9d7eed9e03e53415d37aa96045",
            })
          : makeRequest({ payment: f.mutate(nonce) });

      // For non-fingerprint mutations that fail candidate parse against structure
      // (e.g. inconsistent auth), binding rule still applies when parse succeeds.
      const res2 = await engine.handle(second);
      expect(res2.httpStatus).toBe(409);
      expect(res2.code).toBe("authorization_binding_mismatch");
      expect(res2.body).toMatchObject({ field: f.field });
      expect(gateway.settleCalls).toHaveLength(settleBefore);
      expect(upstream.calls).toHaveLength(upstreamBefore);
      const rowAfter = ledger.getByNonce(nonce)!;
      expect(rowAfter.paymentState).toBe(rowBefore.paymentState);
      expect(rowAfter.fulfillmentState).toBe(rowBefore.fulfillmentState);
      expect(rowAfter.attemptCounter).toBe(rowBefore.attemptCounter);
    });
  }

  it("asset change inside payment.accepted is binding mismatch", async () => {
    const { engine, gateway, upstream } = makeSeller();
    const nonce = fakeNonce(220);
    await engine.handle(makeRequest({ payment: makePayment({ nonce }) }));
    const settleBefore = gateway.settleCalls.length;
    const mutated = makePayment({
      nonce,
      asset: "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    });
    const res = await engine.handle(makeRequest({ payment: mutated }));
    expect(res.code).toBe("authorization_binding_mismatch");
    expect(res.body).toMatchObject({ field: "asset" });
    expect(gateway.settleCalls).toHaveLength(settleBefore);
    expect(upstream.calls).toHaveLength(1);
  });
});

void FIXTURE_ASSET;
void FIXTURE_NETWORK;
void FIXTURE_PAY_TO;
void FIXTURE_PAYER;
void FIXTURE_VERIFYING;
