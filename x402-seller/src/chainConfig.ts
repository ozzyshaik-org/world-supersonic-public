/**
 * Official Circle Gateway chain configuration used by this seller.
 *
 * Source (re-checked 2026-11-06):
 * https://developers.circle.com/gateway/references/contract-addresses
 * Testnet contract addresses → GatewayWallet → Arc Testnet:
 *   0x0077777d7EBA4688BDeF3E311b846F25870A19B9
 *
 * Circle EIP-3009 signing howto also says verifyingContract comes from
 * getVerifyingContract() / 402 accepts[].extra.verifyingContract:
 * https://developers.circle.com/gateway/nanopayments/howtos/eip-3009-signing
 */

export const ARC_TESTNET_CAIP2 = "eip155:5042002";

/** GatewayWallet on Arc Testnet (Circle official contract-addresses page). */
export const ARC_TESTNET_GATEWAY_WALLET =
  "0x0077777d7EBA4688BDeF3E311b846F25870A19B9";

export const GATEWAY_WALLET_BATCHED_EXTRA = {
  name: "GatewayWalletBatched",
  version: "1",
} as const;

/**
 * Circle rejects authorizations with validity shorter than 3 days
 * (errorReason: authorization_validity_too_short).
 * Source: https://developers.circle.com/gateway/nanopayments/howtos/eip-3009-signing
 * (Warning: validBefore must be at least 3 days in the future) — re-checked 2026-11-06.
 */
export const MIN_VALIDITY_WINDOW_SECONDS = 3 * 24 * 60 * 60; // 259200
