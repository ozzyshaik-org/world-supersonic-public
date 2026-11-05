import { describe, expect, it } from "vitest";
import { computeFingerprintV1 } from "../src/fingerprint.js";
import { createHash } from "node:crypto";

const ROUTES: Record<string, string[]> = {
  "/v1/balance": ["address"],
  "/v1/transactions": ["address", "limit"],
  "/v1/block/latest": [],
  "/v1/defi/events": ["contract", "type"],
};

function fp(path: string, rawQuery: string) {
  return computeFingerprintV1({
    method: "GET",
    path,
    rawQuery,
    bodyByteLength: 0,
    allowedQueryParams: ROUTES[path] ?? [],
    pathAllowed: Object.prototype.hasOwnProperty.call(ROUTES, path),
  });
}

describe("fingerprint v1 vectors (spec 01)", () => {
  const vectors: Array<{
    n: number;
    path: string;
    rawQuery: string;
    canonicalQuery: string;
    hash: string;
  }> = [
    {
      n: 1,
      path: "/v1/balance",
      rawQuery: "address=0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
      canonicalQuery: "address=0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
      hash: "ba886fc0227c7de9fec80680d996b742ce885d36acd97586a5356cd5a8beec59",
    },
    {
      n: 2,
      path: "/v1/transactions",
      rawQuery: "limit=10&address=0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
      canonicalQuery: "address=0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045&limit=10",
      hash: "d3314b3faa2a2a11d83b5f514d1d1ec618962591f3dcc770eb2e1e22139ff950",
    },
    {
      n: 3,
      path: "/v1/transactions",
      rawQuery: "address=0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045&limit=",
      canonicalQuery: "address=0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045&limit=",
      hash: "7a16783af6912cf6c09a03ae11ad4ad7171ef16eecd440a27f41273d006f10ca",
    },
    {
      n: 4,
      path: "/v1/block/latest",
      rawQuery: "",
      canonicalQuery: "",
      hash: "e8e05191f29720d69a7662f65ca8c0b9076e8dadfeb9eec249ef884c94f9f317",
    },
    {
      n: 5,
      path: "/v1/defi/events",
      rawQuery: "type=a%20b",
      canonicalQuery: "type=a%20b",
      hash: "04d6ea2c43828ae69a7f7bf04d7430a9a53e785a3f9fb1eadabf2fd0ec18c183",
    },
    {
      n: 6,
      path: "/v1/defi/events",
      rawQuery: "type=a+b",
      canonicalQuery: "type=a%2Bb",
      hash: "e3ab72ae336a01e60dafcc38094db160bc37f27a93720962fb429bd7d7651acf",
    },
    {
      n: 7,
      path: "/v1/defi/events",
      rawQuery: "type=a%0ab",
      canonicalQuery: "type=a%0Ab",
      hash: "1a373336c7664e4e6f9a3e5940ce276b61420547766db6c9f973ceb1fa9beace",
    },
    {
      n: 8,
      path: "/v1/defi/events",
      rawQuery: "type=%c3%a9",
      canonicalQuery: "type=%C3%A9",
      hash: "4a5d45e031dd0ccc18eeaa6fd7972502cdda7d4432aa88598369cd7e452a8def",
    },
    {
      n: 9,
      path: "/v1/balance",
      rawQuery: "address=0xd8da6bf26964af9d7eed9e03e53415d37aa96045",
      canonicalQuery: "address=0xd8da6bf26964af9d7eed9e03e53415d37aa96045",
      hash: "6acd3e7a7953ad7ded8fa951d93e494a3e4c1976a6d8750f095bb218da39bc31",
    },
    {
      n: 10,
      path: "/v1/balance",
      rawQuery: "address=vitalik.eth",
      canonicalQuery: "address=vitalik.eth",
      hash: "36407a0aefb250af55ad8cd129236a8b87d9a19481733d76951522fa1f2f4d0d",
    },
    {
      n: 11,
      path: "/v1/balance",
      rawQuery: "address=Vitalik.eth",
      canonicalQuery: "address=Vitalik.eth",
      hash: "07d110ae01fb259a82cba9acdad88f147257aba96f0e154370476ebc1580e39e",
    },
  ];

  for (const v of vectors) {
    it(`vector ${v.n}`, () => {
      const r = fp(v.path, v.rawQuery);
      expect(r.ok).toBe(true);
      if (!r.ok) return;
      expect(r.canonicalQuery).toBe(v.canonicalQuery);
      expect(r.fingerprint).toBe(v.hash);
      const recomputed = createHash("sha256")
        .update(r.canonicalString, "utf8")
        .digest("hex");
      expect(recomputed).toBe(v.hash);
    });
  }

  it("vector 2 order independence and percent-encoded limit", () => {
    const a = fp(
      "/v1/transactions",
      "address=0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045&limit=10",
    );
    const b = fp(
      "/v1/transactions",
      "address=0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045&limit=%31%30",
    );
    expect(a.ok && b.ok).toBe(true);
    if (a.ok && b.ok) {
      expect(a.fingerprint).toBe(b.fingerprint);
      expect(a.fingerprint).toBe(
        "d3314b3faa2a2a11d83b5f514d1d1ec618962591f3dcc770eb2e1e22139ff950",
      );
    }
  });
});

describe("fingerprint rejection vectors", () => {
  it("R1 duplicate name → 400", () => {
    const r = fp("/v1/transactions", "limit=1&limit=2");
    expect(r).toMatchObject({ ok: false, httpStatus: 400, reason: "duplicate name" });
  });
  it("R2 empty segment → 400", () => {
    const r = fp(
      "/v1/transactions",
      "address=0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045&&limit=1",
    );
    expect(r).toMatchObject({ ok: false, httpStatus: 400, reason: "empty segment" });
  });
  it("R3 segment without = → 400", () => {
    const r = fp("/v1/transactions", "limit");
    expect(r).toMatchObject({ ok: false, httpStatus: 400 });
  });
  it("R4 bad percent escape → 400", () => {
    const r = fp("/v1/defi/events", "type=%zz");
    expect(r).toMatchObject({ ok: false, httpStatus: 400, reason: "bad percent escape" });
  });
  it("R5 not valid UTF-8 → 400", () => {
    const r = fp("/v1/defi/events", "type=%FF");
    expect(r).toMatchObject({ ok: false, httpStatus: 400, reason: "not valid UTF-8" });
  });
  it("R6 trailing slash path → 404", () => {
    const r = computeFingerprintV1({
      method: "GET",
      path: "/v1/balance/",
      rawQuery: "address=0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
      bodyByteLength: 0,
      allowedQueryParams: ["address"],
      pathAllowed: false,
    });
    expect(r).toMatchObject({ ok: false, httpStatus: 404 });
  });
  it("R7 undocumented parameter → 400", () => {
    const r = fp(
      "/v1/balance",
      "address=0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045&x=1",
    );
    expect(r).toMatchObject({ ok: false, httpStatus: 400, reason: "undocumented parameter" });
  });
  it("R8 body not empty → 400", () => {
    const r = computeFingerprintV1({
      method: "GET",
      path: "/v1/balance",
      rawQuery: "address=0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
      bodyByteLength: 1,
      allowedQueryParams: ["address"],
      pathAllowed: true,
    });
    expect(r).toMatchObject({ ok: false, httpStatus: 400, reason: "body not empty" });
  });
  it("R9 POST → 405", () => {
    const r = computeFingerprintV1({
      method: "POST",
      path: "/v1/balance",
      rawQuery: "",
      bodyByteLength: 0,
      allowedQueryParams: ["address"],
      pathAllowed: true,
    });
    expect(r).toMatchObject({ ok: false, httpStatus: 405 });
  });
});
