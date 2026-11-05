/**
 * Ledger and Gateway types for the x402 seller (spec 01).
 * Shapes for settle/transfer match Circle Gateway Nanopayments SDK docs
 * (@circle-fin/x402-batching SettleResponse / TransferStatus), re-read for this checkpoint.
 */

export type PaymentState =
  | "submission_pending"
  | "submission_uncertain"
  | "accepted"
  | "rejected"
  | "void";

/** Copied only from a Gateway lookup; never inferred. */
export type GatewayStatus =
  | "received"
  | "batched"
  | "confirmed"
  | "completed"
  | "failed";

export type FulfillmentState =
  | "not_started"
  | "in_progress"
  | "fulfilled"
  | "retryable_failure"
  | "failed_permanent";

export type IncidentFlag =
  | "fulfilled_but_unpaid"
  | "paid_unfulfilled"
  | "submission_unresolved";

export type RouteClass = "open" | "agent_pass";

export interface PriceTableEntry {
  path: string;
  /** USDC atomic base units as an integer string (6 decimals). */
  price: string;
  routeClass: RouteClass;
  /** Documented query parameter names from docs/openapi.yaml. */
  queryParams: readonly string[];
}

export interface AuthorizationBinding {
  authKey: string;
  nonce: string;
  network: string;
  asset: string;
  payTo: string;
  price: string;
  payer: string;
  validAfter: number;
  validBefore: number;
  fingerprint: string;
}

export interface SettleResponse {
  success: boolean;
  errorReason?: string;
  payer?: string;
  /** Transfer UUID on success; empty string on failure (Circle SettleResponse). */
  transaction: string;
}

export type TransferStatus = GatewayStatus;

export interface TransferRecord {
  id: string;
  status: TransferStatus;
  token: "USDC";
  sendingNetwork: string;
  recipientNetwork: string;
  fromAddress: string;
  toAddress: string;
  amount: string;
  nonce: string;
  txHash: string | null;
  createdAt: string;
  updatedAt: string;
}

/** Circle-documented settle errorReason values treated as definite rejection (HTTP 200 success:false or HTTP 400). */
export const DEFINITE_SETTLE_ERROR_REASONS = [
  "unsupported_scheme",
  "unsupported_network",
  "unsupported_asset",
  "invalid_payload",
  "address_mismatch",
  "amount_mismatch",
  "invalid_signature",
  "authorization_not_yet_valid",
  "authorization_expired",
  "authorization_validity_too_short",
  "self_transfer",
  "insufficient_balance",
  "unsupported_domain",
  "wallet_not_found",
] as const;

/**
 * nonce_already_used is NOT a definite rejection for an uncertain/retry path:
 * spec rule 4 treats it as uncertain and reconciles.
 */
export const UNCERTAIN_SETTLE_ERROR_REASONS = [
  "nonce_already_used",
  "unexpected_error",
] as const;

export interface PaymentPayloadAuthorization {
  from: string;
  to: string;
  value: string;
  validAfter: string | number;
  validBefore: string | number;
  nonce: string;
}

export interface PaymentPayload {
  x402Version?: number;
  scheme?: string;
  network?: string;
  payload: {
    signature?: string;
    authorization: PaymentPayloadAuthorization;
  };
}

export interface PaymentRequirements {
  scheme: string;
  network: string;
  asset: string;
  amount: string;
  payTo: string;
  maxTimeoutSeconds?: number;
  extra?: {
    name?: string;
    version?: string;
    verifyingContract?: string;
  };
}

export interface SellerRequest {
  method: string;
  path: string;
  /** Raw query string without leading '?'; empty if none. */
  rawQuery: string;
  /** Body byte length; committed flow requires zero. */
  bodyByteLength: number;
  /** Decoded payment payload, or null when absent (402 challenge). */
  payment: PaymentPayload | null;
}

export type SellerResultCode =
  | "payment_required"
  | "ok"
  | "payment_rejected"
  | "payment_failed"
  | "payment_submission_unresolved"
  | "authorization_binding_mismatch"
  | "authorization_void"
  | "paid_unfulfilled"
  | "bad_request"
  | "method_not_allowed"
  | "not_found"
  | "upstream_error";

export interface SellerResult {
  httpStatus: number;
  code: SellerResultCode;
  body?: unknown;
  paymentRequirements?: PaymentRequirements;
  authKey?: string;
  gatewayErrorReason?: string;
}

export interface StoredResponse {
  status: number;
  body: unknown;
  contentType: string;
}

export interface Receipt {
  authKey: string;
  gatewayTransferId: string | null;
  fingerprint: string;
  fulfilledAt: string;
}

export interface LedgerRow extends AuthorizationBinding {
  paymentState: PaymentState;
  gatewayStatus: GatewayStatus | null;
  fulfillmentState: FulfillmentState;
  attemptCounter: number;
  attemptAt: string | null;
  leaseOwner: string | null;
  leaseExpiresAt: number | null;
  storedResponse: StoredResponse | null;
  receipt: Receipt | null;
  gatewayTransferId: string | null;
  gatewayError: string | null;
  incidents: IncidentFlag[];
  /** Opaque; never logged or returned to clients. */
  paymentPayload: PaymentPayload;
  createdAt: string;
  updatedAt: string;
}

/** Only `completed` is locally settled (stricter than Circle seller how-to). */
export function isLocallySettled(status: GatewayStatus | null): boolean {
  return status === "completed";
}

/** `received` is an intermediate state and is NOT batching evidence. */
export function isDirectBatchingEvidence(status: GatewayStatus | null): boolean {
  return status === "batched" || status === "confirmed";
}

export function gatewayStatusRank(status: GatewayStatus | null): number {
  if (status === null) return 0;
  const order: Record<GatewayStatus, number> = {
    received: 1,
    batched: 2,
    confirmed: 3,
    completed: 4,
    failed: 5,
  };
  return order[status];
}
