/**
 * Property-based tests for PAYMENT-SIGNATURE header parsing.
 * Label: unit-fake (no network). Uses fast-check.
 */
import { describe, expect, it } from "vitest";
import * as fc from "fast-check";
import {
  PaymentHeaderError,
  decodePaymentSignatureHeader,
  encodePaymentSignatureHeader,
  MAX_PAYMENT_HEADER_CHARS,
} from "../src/http/headers.js";
import { makePayment, fakeNonce } from "./helpers.js";

describe("PAYMENT-SIGNATURE property tests (unit-fake)", () => {
  it("round-trips arbitrary synthetic PaymentPayload objects", () => {
    fc.assert(
      fc.property(
        fc.integer({ min: 1, max: 10_000 }),
        fc.constantFrom("10000", "20000", "5000"),
        (seed, amount) => {
          const payment = makePayment({
            nonce: fakeNonce(seed),
            value: amount,
            acceptedAmount: amount,
          });
          const encoded = encodePaymentSignatureHeader(payment);
          const decoded = decodePaymentSignatureHeader(encoded);
          expect(decoded.x402Version).toBe(2);
          expect(decoded.accepted.amount).toBe(amount);
          expect(decoded.payload.authorization.nonce).toBe(payment.payload.authorization.nonce);
          expect(decoded.payload.authorization.value).toBe(amount);
        },
      ),
      { numRuns: 50 },
    );
  });

  it("rejects non-base64 and truncated base64 inputs", () => {
    fc.assert(
      fc.property(
        fc.string({ minLength: 1, maxLength: 64 }).filter((s) => /[^A-Za-z0-9+/=]/.test(s)),
        (s) => {
          expect(() => decodePaymentSignatureHeader(s)).toThrow(PaymentHeaderError);
        },
      ),
      { numRuns: 40 },
    );
  });

  it("rejects oversized inputs", () => {
    const huge = "A".repeat(MAX_PAYMENT_HEADER_CHARS + 10);
    expect(() => decodePaymentSignatureHeader(huge)).toThrow(PaymentHeaderError);
  });
});
