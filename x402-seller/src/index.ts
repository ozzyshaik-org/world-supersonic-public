export { buildAuthKey, parseAuthKey, normalizeNonce, normalizePayer, AuthKeyError } from "./authKey.js";
export { computeFingerprintV1, FINGERPRINT_VERSION_LABEL } from "./fingerprint.js";
export type { FingerprintResult } from "./fingerprint.js";
export {
  SellerEngine,
  isLocallySettled,
  isDirectBatchingEvidence,
} from "./seller.js";
export type { SellerConfig, SellerDeps } from "./seller.js";
export { MemoryLedger, LedgerConflictError } from "./ledger/memoryLedger.js";
export { FakeGatewayClient, makeFakeTransfer } from "./gateway/fakeGateway.js";
export type { FakeSettleMode, FakeGatewayOptions } from "./gateway/fakeGateway.js";
export type { GatewayClient } from "./gateway/client.js";
export { CircleBatchFacilitatorAdapter } from "./gateway/circleAdapter.js";
export type { CircleAdapterDeps, CircleFacilitatorLike, CircleTransferLookup } from "./gateway/circleAdapter.js";
export { FakeUpstreamClient } from "./upstream/fakeUpstream.js";
export type { UpstreamClient, UpstreamRequest, UpstreamResponse } from "./upstream/client.js";
export { DEFAULT_PRICE_TABLE, lookupPrice } from "./priceTable.js";
export {
  ARC_TESTNET_CAIP2,
  ARC_TESTNET_GATEWAY_WALLET,
  MIN_VALIDITY_WINDOW_SECONDS,
} from "./chainConfig.js";
export {
  parseStrictSafeInteger,
  parseCandidateBinding,
  validateNewPayment,
  PaymentValidationError,
  bindingMismatchField,
} from "./paymentValidation.js";
export {
  transferMatchesBinding,
  classifySearchHits,
  classifyTransferById,
} from "./transferMatch.js";
export type {
  PaymentState,
  GatewayStatus,
  FulfillmentState,
  IncidentFlag,
  LedgerRow,
  SellerRequest,
  SellerResult,
  PaymentPayload,
  PaymentRequirements,
  SettleResponse,
  TransferRecord,
  AuthorizationBinding,
  PriceTableEntry,
} from "./types.js";
export {
  DEFINITE_SETTLE_ERROR_REASONS,
  UNCERTAIN_SETTLE_ERROR_REASONS,
  gatewayStatusRank,
  isKnownGatewayStatus,
  KNOWN_GATEWAY_STATUSES,
} from "./types.js";
