import { describe, expect, it } from "vitest";
import { buildAuthKey, parseAuthKey, AuthKeyError } from "../src/authKey.js";
import { FIXTURE_NETWORK, FIXTURE_PAYER, fakeNonce } from "./helpers.js";

describe("auth_key", () => {
  it("formats as network|payer|nonce with lowercase payer and nonce", () => {
    const nonce = fakeNonce(42);
    const key = buildAuthKey(
      FIXTURE_NETWORK,
      "0xAbCdEf0123456789AbCdEf0123456789AbCdEf01",
      nonce.toUpperCase().replace("0X", "0x"),
    );
    expect(key).toBe(
      `${FIXTURE_NETWORK}|0xabcdef0123456789abcdef0123456789abcdef01|${nonce}`,
    );
    const parsed = parseAuthKey(key);
    expect(parsed.network).toBe(FIXTURE_NETWORK);
    expect(parsed.payer).toBe("0xabcdef0123456789abcdef0123456789abcdef01");
    expect(parsed.nonce).toBe(nonce);
  });

  it("rejects non-32-byte nonce", () => {
    expect(() => buildAuthKey(FIXTURE_NETWORK, FIXTURE_PAYER, "0xdead")).toThrow(AuthKeyError);
  });

  it("rejects invalid payer", () => {
    expect(() => buildAuthKey(FIXTURE_NETWORK, "not-an-address", fakeNonce())).toThrow(
      AuthKeyError,
    );
  });
});
