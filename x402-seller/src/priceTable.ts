import type { PriceTableEntry } from "./types.js";

/**
 * Initial open-route price table (USDC atomic base units).
 * Prices are placeholders for the first checkpoint — commercial docs are not touched.
 * /v1/balance is an application address-balance route, NOT a wallet/Gateway balance.
 */
export const DEFAULT_PRICE_TABLE: readonly PriceTableEntry[] = [
  {
    path: "/v1/balance",
    price: "10000",
    routeClass: "open",
    queryParams: ["address"],
  },
  {
    path: "/v1/transactions",
    price: "10000",
    routeClass: "open",
    queryParams: ["address", "limit"],
  },
  {
    path: "/v1/block/latest",
    price: "5000",
    routeClass: "open",
    queryParams: [],
  },
  {
    path: "/v1/defi/events",
    price: "10000",
    routeClass: "open",
    queryParams: ["contract", "type"],
  },
  {
    path: "/v1/mev/analytics",
    price: "20000",
    routeClass: "open",
    queryParams: ["type", "block"],
  },
] as const;

export function lookupPrice(
  table: readonly PriceTableEntry[],
  path: string,
): PriceTableEntry | undefined {
  return table.find((e) => e.path === path);
}
