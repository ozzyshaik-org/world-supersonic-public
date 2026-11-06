/**
 * Strict x402 v2 payment-structure validation (spec 01).
 * Rejects before ledger reservation / Gateway calls for NEW nonces.
 * For EXISTING nonces, parseCandidateBinding extracts enough to apply the binding rule
 * before ordinary price-table mismatch rejection.
 */

import { assertNonce32Bytes, buildAuthKey, AuthKeyError } from "./authKey.js";
import {
  GATEWAY_WALLET_BATCHED_EXTRA,
  MIN_VALIDITY_WINDOW_SECONDS,
} from "./chainConfig.js";
import type {
  AuthorizationBinding,
  PaymentPayload,
  PaymentRequirements,
  PriceTableEntry,
} from "./types.js";

export class PaymentValidationError extends Error {
  readonly httpStatus: number;
  constructor(message: string, httpStatus = 400) {
    super(message);
    this.name = "PaymentValidationError";
    this.httpStatus = httpStatus;
  }
}

/**
 * Parse a strict non-negative safe integer from string | number.
 * Rejects: fractional numbers, unsafe integers, malformed strings like "1900000000junk",
 * empty strings, hex, scientific notation with fraction, leading/trailing whitespace tricks.
 */
export function parseStrictSafeInteger(value: unknown, field: string): number {
  if (typeof value === "number") {
    if (!Number.isInteger(value) || !Number.isSafeInteger(value) || value < 0) {
      throw new PaymentValidationError(`invalid ${field}: not a non-negative safe integer`);
    }
    return value;
  }
  if (typeof value !== "string") {
    throw new PaymentValidationError(`invalid ${field}: must be string or number`);
  }
  if (value.length === 0) {
    throw new PaymentValidationError(`invalid ${field}: empty string`);
  }
  // Strict decimal digits only (no sign, no whitespace, no fraction, no junk suffix).
  if (!/^(0|[1-9][0-9]*)$/.test(value)) {
    throw new PaymentValidationError(`invalid ${field}: malformed integer string`);
  }
  let n: number;
  try {
    n = Number(value);
  } catch {
    throw new PaymentValidationError(`invalid ${field}: not parseable`);
  }
  if (!Number.isSafeInteger(n) || n < 0) {
    throw new PaymentValidationError(`invalid ${field}: not a non-negative safe integer`);
  }
  // Round-trip: Number→string must match for values that lose precision... already checked safe.
  if (String(n) !== value && !(value === "0" && n === 0)) {
    // Leading zeros already rejected by regex except single 0.
    throw new PaymentValidationError(`invalid ${field}: not a non-negative safe integer`);
  }
  return n;
}

function lower0x(addr: string): string {
  return addr.toLowerCase();
}

function requireString(value: unknown, field: string): string {
  if (typeof value !== "string" || value.length === 0) {
    throw new PaymentValidationError(`missing or invalid ${field}`);
  }
  return value;
}

export interface SellerTerms {
  network: string;
  asset: string;
  payTo: string;
  verifyingContract: string;
  /** Unix seconds "now" for validity-window checks. */
  nowSec: number;
}

/**
 * Parse enough of a payment to construct a candidate AuthorizationBinding.
 * Does NOT compare against the price table / configured route terms (except fingerprint input).
 * Used for EXISTING-nonce binding comparison before price-table mismatch rejection.
 */
export function parseCandidateBinding(
  payment: PaymentPayload,
  fingerprint: string,
): AuthorizationBinding {
  if (payment == null || typeof payment !== "object") {
    throw new PaymentValidationError("missing payment payload");
  }
  if (payment.x402Version !== 2) {
    throw new PaymentValidationError("x402Version must be 2");
  }
  const accepted = payment.accepted;
  if (accepted == null || typeof accepted !== "object") {
    throw new PaymentValidationError("missing payment.accepted requirements object");
  }
  const scheme = requireString(accepted.scheme, "accepted.scheme");
  const network = requireString(accepted.network, "accepted.network");
  const asset = requireString(accepted.asset, "accepted.asset");
  const amount = requireString(accepted.amount, "accepted.amount");
  const payToAccepted = requireString(accepted.payTo, "accepted.payTo");
  if (typeof accepted.maxTimeoutSeconds !== "number" || !Number.isFinite(accepted.maxTimeoutSeconds)) {
    throw new PaymentValidationError("missing or invalid accepted.maxTimeoutSeconds");
  }
  const extra = accepted.extra;
  if (extra == null || typeof extra !== "object") {
    throw new PaymentValidationError("missing accepted.extra (GatewayWalletBatched)");
  }
  if (extra.name !== GATEWAY_WALLET_BATCHED_EXTRA.name) {
    throw new PaymentValidationError("accepted.extra.name must be GatewayWalletBatched");
  }
  requireString(extra.version, "accepted.extra.version");
  requireString(extra.verifyingContract, "accepted.extra.verifyingContract");

  const payload = payment.payload;
  if (payload == null || typeof payload !== "object") {
    throw new PaymentValidationError("missing payload");
  }
  requireString(payload.signature, "payload.signature");
  const auth = payload.authorization;
  if (auth == null || typeof auth !== "object") {
    throw new PaymentValidationError("missing payload.authorization");
  }
  const from = requireString(auth.from, "authorization.from");
  const to = requireString(auth.to, "authorization.to");
  const value = requireString(auth.value, "authorization.value");
  let nonce: string;
  try {
    nonce = assertNonce32Bytes(requireString(auth.nonce, "authorization.nonce"));
  } catch (e) {
    if (e instanceof AuthKeyError) {
      throw new PaymentValidationError(e.message);
    }
    throw e;
  }
  const validAfter = parseStrictSafeInteger(auth.validAfter, "authorization.validAfter");
  const validBefore = parseStrictSafeInteger(auth.validBefore, "authorization.validBefore");
  if (validAfter > validBefore) {
    throw new PaymentValidationError("validAfter must be <= validBefore");
  }

  // authorization.to/value must be consistent with accepted.payTo/amount
  if (lower0x(to) !== lower0x(payToAccepted)) {
    throw new PaymentValidationError("authorization.to inconsistent with accepted.payTo");
  }
  if (value !== amount) {
    throw new PaymentValidationError("authorization.value inconsistent with accepted.amount");
  }

  const payer = lower0x(from);
  const payTo = lower0x(payToAccepted);
  let authKey: string;
  try {
    authKey = buildAuthKey(network, payer, nonce);
  } catch (e) {
    if (e instanceof AuthKeyError) {
      throw new PaymentValidationError(e.message);
    }
    throw e;
  }

  // Silence unused for scheme in binding — scheme is validated present; binding uses network etc.
  void scheme;

  return {
    authKey,
    nonce,
    network,
    asset: lower0x(asset),
    payTo,
    price: amount,
    payer,
    validAfter,
    validBefore,
    fingerprint,
  };
}

/**
 * Full validation for a NEW nonce against configured seller terms and the price-table entry.
 * Throws PaymentValidationError on any failure (caller maps to 400 before reservation).
 */
export function validateNewPayment(
  payment: PaymentPayload,
  entry: PriceTableEntry,
  fingerprint: string,
  terms: SellerTerms,
): AuthorizationBinding {
  const binding = parseCandidateBinding(payment, fingerprint);

  if (payment.accepted.scheme !== "exact") {
    throw new PaymentValidationError("accepted.scheme must be exact");
  }
  if (binding.network !== terms.network) {
    throw new PaymentValidationError("network mismatch");
  }
  if (binding.asset !== lower0x(terms.asset)) {
    throw new PaymentValidationError("asset mismatch");
  }
  if (binding.payTo !== lower0x(terms.payTo)) {
    throw new PaymentValidationError("payTo mismatch");
  }
  if (binding.price !== entry.price) {
    throw new PaymentValidationError("amount mismatch");
  }
  const verifying = payment.accepted.extra.verifyingContract;
  if (lower0x(verifying) !== lower0x(terms.verifyingContract)) {
    throw new PaymentValidationError("verifyingContract mismatch");
  }

  // Circle validity-window: validBefore must be at least 3 days ahead of now.
  if (binding.validBefore < terms.nowSec + MIN_VALIDITY_WINDOW_SECONDS) {
    throw new PaymentValidationError(
      "authorization_validity_too_short: validBefore must be at least 3 days ahead",
    );
  }
  if (binding.validAfter > terms.nowSec) {
    // Not yet valid — still a structure/window refusal before settle (Gateway would reject).
    // Spec: validate before settle; refuse with 4xx before ledger.
    throw new PaymentValidationError("authorization_not_yet_valid");
  }

  return binding;
}

export function buildPaymentRequirements(
  entry: PriceTableEntry,
  terms: Omit<SellerTerms, "nowSec">,
): PaymentRequirements {
  return {
    scheme: "exact",
    network: terms.network,
    asset: terms.asset,
    amount: entry.price,
    payTo: terms.payTo,
    maxTimeoutSeconds: MIN_VALIDITY_WINDOW_SECONDS + 86400, // 4 days display default
    extra: {
      name: "GatewayWalletBatched",
      version: GATEWAY_WALLET_BATCHED_EXTRA.version,
      verifyingContract: terms.verifyingContract,
    },
  };
}

export function bindingMismatchField(
  stored: AuthorizationBinding,
  incoming: AuthorizationBinding,
): string | null {
  const fields: (keyof AuthorizationBinding)[] = [
    "payer",
    "network",
    "asset",
    "payTo",
    "price",
    "validAfter",
    "validBefore",
    "fingerprint",
  ];
  for (const f of fields) {
    if (stored[f] !== incoming[f]) return f;
  }
  return null;
}
