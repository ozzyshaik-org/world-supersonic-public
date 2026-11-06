/**
 * x402 v2 HTTP header codecs (PAYMENT-REQUIRED / PAYMENT-SIGNATURE / PAYMENT-RESPONSE).
 * Wire format: standard base64 (with padding) of UTF-8 JSON.
 * Sources: Circle x402 concepts; x402 transports-v2/http.md; Circle seller quickstart.
 */

import type { PaymentPayload, PaymentRequirements, SettleResponse } from "../types.js";

export const HEADER_PAYMENT_REQUIRED = "PAYMENT-REQUIRED";
export const HEADER_PAYMENT_SIGNATURE = "PAYMENT-SIGNATURE";
export const HEADER_PAYMENT_RESPONSE = "PAYMENT-RESPONSE";

/** Refuse oversized payment headers before decode (defence in depth). */
export const MAX_PAYMENT_HEADER_CHARS = 65_536;

/**
 * PaymentRequired envelope (x402 v2 / Circle seller quickstart body shape).
 * Canonical HTTP location is the PAYMENT-REQUIRED header (base64); this package
 * also emits the same JSON as the 402 response body for machine clients that
 * read bodies (Circle's non-Express seller example uses the body).
 */
export interface PaymentRequiredEnvelope {
  x402Version: 2;
  accepts: PaymentRequirements[];
}

export class PaymentHeaderError extends Error {
  readonly httpStatus = 400;
  readonly reason: string;
  constructor(reason: string) {
    super(reason);
    this.name = "PaymentHeaderError";
    this.reason = reason;
  }
}

export function buildPaymentRequiredEnvelope(
  requirements: PaymentRequirements,
): PaymentRequiredEnvelope {
  return { x402Version: 2, accepts: [requirements] };
}

export function encodePaymentRequiredHeader(envelope: PaymentRequiredEnvelope): string {
  return Buffer.from(JSON.stringify(envelope), "utf8").toString("base64");
}

export function encodePaymentSignatureHeader(payload: PaymentPayload): string {
  return Buffer.from(JSON.stringify(payload), "utf8").toString("base64");
}

/**
 * Settlement confirmation for PAYMENT-RESPONSE.
 * Shape mirrors Circle SettleResponse fields we already model; full wire
 * SettlementResponse resource/error extensions remain UNRESOLVED in
 * docs/CIRCLE_RECHECK.md when not supplied by the engine result.
 */
export function encodePaymentResponseHeader(
  response: Pick<SettleResponse, "success" | "transaction" | "errorReason" | "payer">,
): string {
  return Buffer.from(JSON.stringify(response), "utf8").toString("base64");
}

function decodeBase64Json(raw: string, label: string): unknown {
  if (raw.length > MAX_PAYMENT_HEADER_CHARS) {
    throw new PaymentHeaderError(`${label}_too_large`);
  }
  let jsonText: string;
  try {
    const buf = Buffer.from(raw, "base64");
    // Reject non-base64 garbage: Node Buffer.from is permissive; require round-trip
    // of the alphabet (ignore whitespace) roughly by ensuring we got some bytes
    // when input was non-empty, and that re-encoding is consistent enough.
    if (raw.replace(/\s+/g, "").length === 0) {
      throw new PaymentHeaderError(`${label}_empty`);
    }
    if (buf.length === 0 && raw.replace(/\s+/g, "").length > 0) {
      // all-padding or invalid → empty buffer with non-empty input
      throw new PaymentHeaderError(`${label}_invalid_base64`);
    }
    // Detect clearly invalid base64 characters (excluding whitespace).
    if (/[^A-Za-z0-9+/=\s]/.test(raw)) {
      throw new PaymentHeaderError(`${label}_invalid_base64`);
    }
    jsonText = buf.toString("utf8");
  } catch (e) {
    if (e instanceof PaymentHeaderError) throw e;
    throw new PaymentHeaderError(`${label}_invalid_base64`);
  }
  try {
    return JSON.parse(jsonText) as unknown;
  } catch {
    throw new PaymentHeaderError(`${label}_invalid_json`);
  }
}

/**
 * Decode PAYMENT-SIGNATURE into a PaymentPayload-shaped object.
 * Structural validation of payment fields is deferred to SellerEngine /
 * paymentValidation (this layer only guarantees base64 JSON object).
 */
export function decodePaymentSignatureHeader(raw: string): PaymentPayload {
  const value = decodeBase64Json(raw, "payment_signature");
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    throw new PaymentHeaderError("payment_signature_not_object");
  }
  return value as PaymentPayload;
}

/** Case-insensitive header lookup (Node lowercases incoming request headers). */
export function getHeader(
  headers: Record<string, string | string[] | undefined>,
  name: string,
): string | undefined {
  const want = name.toLowerCase();
  for (const [k, v] of Object.entries(headers)) {
    if (k.toLowerCase() === want) {
      if (Array.isArray(v)) return v[0];
      return v;
    }
  }
  return undefined;
}
