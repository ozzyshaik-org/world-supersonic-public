/**
 * unit-fake — payment header encode/decode (no network).
 */
import { describe, expect, it } from "vitest";
import {
  PaymentHeaderError,
  buildPaymentRequiredEnvelope,
  decodePaymentSignatureHeader,
  encodePaymentRequiredHeader,
  encodePaymentSignatureHeader,
  MAX_PAYMENT_HEADER_CHARS,
} from "../src/http/headers.js";
import { makePayment } from "./helpers.js";

describe("http payment headers (unit-fake)", () => {
  it("round-trips PAYMENT-SIGNATURE base64 JSON", () => {
    const payment = makePayment({ value: "20000" });
    const encoded = encodePaymentSignatureHeader(payment);
    const decoded = decodePaymentSignatureHeader(encoded);
    expect(decoded.x402Version).toBe(2);
    expect(decoded.accepted.amount).toBe("20000");
    expect(decoded.payload.authorization.nonce).toBe(payment.payload.authorization.nonce);
  });

  it("round-trips PAYMENT-REQUIRED envelope", () => {
    const payment = makePayment({ value: "20000" });
    const envelope = buildPaymentRequiredEnvelope(payment.accepted);
    const encoded = encodePaymentRequiredHeader(envelope);
    const raw = Buffer.from(encoded, "base64").toString("utf8");
    const parsed = JSON.parse(raw) as { x402Version: number; accepts: unknown[] };
    expect(parsed.x402Version).toBe(2);
    expect(parsed.accepts).toHaveLength(1);
    expect(parsed.accepts[0]).toEqual(payment.accepted);
  });

  it("rejects invalid base64", () => {
    expect(() => decodePaymentSignatureHeader("!!!not-base64!!!")).toThrow(PaymentHeaderError);
  });

  it("rejects invalid JSON after base64", () => {
    const bad = Buffer.from("{not-json", "utf8").toString("base64");
    expect(() => decodePaymentSignatureHeader(bad)).toThrow(PaymentHeaderError);
  });

  it("rejects non-object JSON", () => {
    const bad = Buffer.from("[]", "utf8").toString("base64");
    expect(() => decodePaymentSignatureHeader(bad)).toThrow(PaymentHeaderError);
  });

  it("rejects oversized header", () => {
    const huge = "A".repeat(MAX_PAYMENT_HEADER_CHARS + 1);
    expect(() => decodePaymentSignatureHeader(huge)).toThrow(/too_large/);
  });
});
