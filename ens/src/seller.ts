import { getAddress } from "viem";
import type { EnsResolution } from "./resolver.js";

/**
 * ENS text-record schema for an x402 seller (draft, project-defined).
 * Vendor keys use ENSIP-5 reverse-DNS notation under org.x402.*; "url" and "description" are ENSIP-5 global keys.
 */
export const SELLER_KEYS = {
  endpoint: "org.x402.endpoint",
  price: "org.x402.price",
  payTo: "org.x402.payTo",
  network: "org.x402.network",
  url: "url",
  description: "description",
} as const;
export const SELLER_TEXT_KEYS: string[] = Object.values(SELLER_KEYS);

export interface SellerDescriptor {
  name: string;
  endpoint: string; // normalised https URL on an allowlisted host
  price: string; // positive integer string, atomic units
  payTo: `0x${string}`; // EIP-55 checksummed, equals the name's resolved addr
  network: string; // CAIP-2, eip155 namespace
  url: string | null;
  description: string | null;
}

export type SellerErrorCode =
  | "NO_ADDR"
  | "MISSING_FIELD"
  | "ENDPOINT_INVALID"
  | "ENDPOINT_NOT_HTTPS"
  | "ENDPOINT_USERINFO"
  | "ENDPOINT_IP_LITERAL"
  | "ENDPOINT_LOCALHOST"
  | "ENDPOINT_HOST_NOT_ALLOWED"
  | "ENDPOINT_PORT_NOT_ALLOWED"
  | "PAYTO_NOT_CHECKSUMMED"
  | "PAYTO_MISMATCH"
  | "NETWORK_INVALID"
  | "PRICE_INVALID";

export interface SellerError { code: SellerErrorCode; field: string; message: string }
export type SellerParseResult = { ok: true; seller: SellerDescriptor } | { ok: false; error: SellerError };

export interface EndpointPolicy {
  /** Exact, lowercase hostnames that an endpoint may point at. Empty means nothing is allowed. */
  allowedHosts: readonly string[];
  /** Non-default ports permitted per host, e.g. { "api.example.org": [8443] }. */
  allowedPorts?: Readonly<Record<string, readonly number[]>>;
}

const err = (code: SellerErrorCode, field: string, message: string): SellerParseResult => ({
  ok: false,
  error: { code, field, message },
});

const IPV4 = /^\d{1,3}(\.\d{1,3}){3}$/;

/** SSRF guard for an endpoint taken from an ENS record. Returns the normalised URL or a typed error. */
export function checkEndpoint(raw: string, policy: EndpointPolicy): { ok: true; url: string } | { ok: false; error: SellerError } {
  const f = SELLER_KEYS.endpoint;
  const fail = (code: SellerErrorCode, message: string) => ({ ok: false as const, error: { code, field: f, message } });
  if (raw.length > 2048 || /\s/.test(raw)) return fail("ENDPOINT_INVALID", "endpoint too long or contains whitespace");
  let u: URL;
  try {
    u = new URL(raw);
  } catch {
    return fail("ENDPOINT_INVALID", "endpoint is not an absolute URL");
  }
  if (u.protocol !== "https:") return fail("ENDPOINT_NOT_HTTPS", "endpoint must use https");
  if (u.username !== "" || u.password !== "") return fail("ENDPOINT_USERINFO", "endpoint must not contain credentials");
  // WHATWG URL parsing canonicalises decimal/hex/octal IPv4 forms (e.g. 2130706433, 0x7f.1) to dotted quads.
  const host = u.hostname.toLowerCase();
  if (host.startsWith("[") || IPV4.test(host)) return fail("ENDPOINT_IP_LITERAL", "endpoint must use a hostname, not an IP literal");
  const bare = host.endsWith(".") ? host.slice(0, -1) : host;
  if (bare === "localhost" || bare.endsWith(".localhost")) return fail("ENDPOINT_LOCALHOST", "endpoint must not be localhost");
  if (!policy.allowedHosts.includes(host)) return fail("ENDPOINT_HOST_NOT_ALLOWED", `host ${host} is not on the allowlist`);
  if (u.port !== "" && !(policy.allowedPorts?.[host] ?? []).includes(Number(u.port))) {
    return fail("ENDPOINT_PORT_NOT_ALLOWED", `port ${u.port} is not allowed for ${host}`);
  }
  return { ok: true, url: u.toString() };
}

const CAIP2_EIP155 = /^eip155:[1-9][0-9]{0,31}$/; // CAIP-2 namespace:reference, eip155 reference = decimal chain id
const PRICE = /^[1-9][0-9]*$/;
const UINT256_MAX = (1n << 256n) - 1n;

/** Parse resolved ENS records into an x402 seller descriptor. Pure: no network access. */
export function parseSellerRecords(res: Pick<EnsResolution, "name" | "address" | "texts">, policy: EndpointPolicy): SellerParseResult {
  if (res.address === null) return err("NO_ADDR", "addr", "name has no addr record");
  const t = (k: string) => res.texts[k] ?? null;
  for (const k of [SELLER_KEYS.endpoint, SELLER_KEYS.price, SELLER_KEYS.payTo, SELLER_KEYS.network]) {
    if (t(k) === null) return err("MISSING_FIELD", k, `missing text record ${k}`);
  }
  const ep = checkEndpoint(t(SELLER_KEYS.endpoint)!, policy);
  if (!ep.ok) return { ok: false, error: ep.error };

  const payTo = t(SELLER_KEYS.payTo)!;
  let checksummed: `0x${string}`;
  try {
    checksummed = getAddress(payTo);
  } catch {
    return err("PAYTO_NOT_CHECKSUMMED", SELLER_KEYS.payTo, "payTo is not an address");
  }
  if (checksummed !== payTo) return err("PAYTO_NOT_CHECKSUMMED", SELLER_KEYS.payTo, "payTo must be EIP-55 checksummed");
  if (checksummed !== getAddress(res.address)) return err("PAYTO_MISMATCH", SELLER_KEYS.payTo, "payTo must equal the name's addr record");

  const network = t(SELLER_KEYS.network)!;
  if (!CAIP2_EIP155.test(network)) return err("NETWORK_INVALID", SELLER_KEYS.network, "network must be a CAIP-2 eip155 chain id");

  const price = t(SELLER_KEYS.price)!;
  if (!PRICE.test(price) || BigInt(price) > UINT256_MAX) return err("PRICE_INVALID", SELLER_KEYS.price, "price must be a positive integer string");

  return {
    ok: true,
    seller: { name: res.name, endpoint: ep.url, price, payTo: checksummed, network, url: t(SELLER_KEYS.url), description: t(SELLER_KEYS.description) },
  };
}
