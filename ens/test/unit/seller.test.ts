// Evidence tier: unit/local only. Pure parsing; no network, no ENS name involved.
import { describe, expect, it } from "vitest";
import fc from "fast-check";
import { checkEndpoint, parseSellerRecords, SELLER_KEYS as K, type EndpointPolicy } from "../../src/index.js";

// EIP-55 published test vectors (https://eips.ethereum.org/EIPS/eip-55#test-cases)
const EIP55 = "0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed";
const EIP55_2 = "0xfB6916095ca1df60bB79Ce92cE3Ea74c37c5d359";
const policy: EndpointPolicy = { allowedHosts: ["seller.example.org"], allowedPorts: { "seller.example.org": [8443] } };

const base = () => ({
  name: "seller.foo.eth",
  address: EIP55 as `0x${string}`,
  texts: {
    [K.endpoint]: "https://seller.example.org/v1/mev/analytics",
    [K.price]: "1000",
    [K.payTo]: EIP55,
    [K.network]: "eip155:5042002",
    [K.url]: null,
    [K.description]: "MEV analytics",
  } as Record<string, string | null>,
});
const withText = (k: string, v: string | null) => { const b = base(); b.texts[k] = v; return b; };
const code = (r: ReturnType<typeof parseSellerRecords>) => (r.ok ? "OK" : r.error.code);

describe("parseSellerRecords", () => {
  it("returns the hand-written expected descriptor for valid records", () => {
    expect(parseSellerRecords(base(), policy)).toEqual({
      ok: true,
      seller: {
        name: "seller.foo.eth",
        endpoint: "https://seller.example.org/v1/mev/analytics",
        price: "1000",
        payTo: EIP55,
        network: "eip155:5042002",
        url: null,
        description: "MEV analytics",
      },
    });
  });

  it("requires an addr record", () => expect(code(parseSellerRecords({ ...base(), address: null }, policy))).toBe("NO_ADDR"));

  it.each([K.endpoint, K.price, K.payTo, K.network])("requires %s", (k) => {
    const r = parseSellerRecords(withText(k, null), policy);
    expect(r).toMatchObject({ ok: false, error: { code: "MISSING_FIELD", field: k } });
  });

  it.each([
    [EIP55.toLowerCase(), "PAYTO_NOT_CHECKSUMMED"],
    ["0x5AAEB6053F3E94C9B9A09F33669435E7EF1BEAED", "PAYTO_NOT_CHECKSUMMED"],
    ["0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAeD", "PAYTO_NOT_CHECKSUMMED"], // last char case flipped
    ["not-an-address", "PAYTO_NOT_CHECKSUMMED"],
    [EIP55_2, "PAYTO_MISMATCH"],
  ])("payTo %s -> %s", (v, c) => expect(code(parseSellerRecords(withText(K.payTo, v), policy))).toBe(c));

  // CAIP-2 examples from https://github.com/ChainAgnostic/CAIPs/blob/main/CAIPs/caip-2.md
  it.each([
    ["eip155:1", "OK"],
    ["eip155:11155111", "OK"],
    ["bip122:000000000019d6689c085ae165831e93", "NETWORK_INVALID"], // valid CAIP-2, but not EVM
    ["eip155:0", "NETWORK_INVALID"],
    ["eip155:01", "NETWORK_INVALID"],
    ["eip155", "NETWORK_INVALID"],
    ["1", "NETWORK_INVALID"],
    ["EIP155:1", "NETWORK_INVALID"],
  ])("network %s -> %s", (v, c) => expect(code(parseSellerRecords(withText(K.network, v), policy))).toBe(c));

  it.each([
    ["1", "OK"],
    ["115792089237316195423570985008687907853269984665640564039457584007913129639935", "OK"], // 2^256-1
    ["115792089237316195423570985008687907853269984665640564039457584007913129639936", "PRICE_INVALID"], // 2^256
    ["0", "PRICE_INVALID"],
    ["-1", "PRICE_INVALID"],
    ["01", "PRICE_INVALID"],
    ["1.5", "PRICE_INVALID"],
    ["1e3", "PRICE_INVALID"],
    [" 1", "PRICE_INVALID"],
  ])("price %j -> %s", (v, c) => expect(code(parseSellerRecords(withText(K.price, v), policy))).toBe(c));
});

describe("checkEndpoint (SSRF guard)", () => {
  it.each([
    ["https://seller.example.org/x", "OK"],
    ["https://SELLER.example.org/x", "OK"],
    ["https://seller.example.org:8443/x", "OK"],
    ["https://seller.example.org:443/x", "OK"], // default port is dropped by URL parsing
    ["http://seller.example.org/x", "ENDPOINT_NOT_HTTPS"],
    ["ftp://seller.example.org/x", "ENDPOINT_NOT_HTTPS"],
    ["https://user:pw@seller.example.org/x", "ENDPOINT_USERINFO"],
    ["https://user@seller.example.org/x", "ENDPOINT_USERINFO"],
    ["https://127.0.0.1/x", "ENDPOINT_IP_LITERAL"],
    ["https://2130706433/x", "ENDPOINT_IP_LITERAL"], // decimal 127.0.0.1
    ["https://0x7f.1/x", "ENDPOINT_IP_LITERAL"],
    ["https://10.0.0.1/x", "ENDPOINT_IP_LITERAL"],
    ["https://169.254.169.254/latest/meta-data", "ENDPOINT_IP_LITERAL"],
    ["https://[::1]/x", "ENDPOINT_IP_LITERAL"],
    ["https://[fd00::1]/x", "ENDPOINT_IP_LITERAL"],
    ["https://localhost/x", "ENDPOINT_LOCALHOST"],
    ["https://api.localhost/x", "ENDPOINT_LOCALHOST"],
    ["https://seller.example.org./x", "ENDPOINT_HOST_NOT_ALLOWED"],
    ["https://evil.example.org/x", "ENDPOINT_HOST_NOT_ALLOWED"],
    ["https://seller.example.org.evil.com/x", "ENDPOINT_HOST_NOT_ALLOWED"],
    ["https://seller.example.org:8080/x", "ENDPOINT_PORT_NOT_ALLOWED"],
    ["not a url", "ENDPOINT_INVALID"],
    ["/relative", "ENDPOINT_INVALID"],
  ])("%s -> %s", (u, c) => {
    const r = checkEndpoint(u, policy);
    expect(r.ok ? "OK" : r.error.code).toBe(c);
  });

  it("rejects everything when the allowlist is empty", () => {
    expect(checkEndpoint("https://seller.example.org/x", { allowedHosts: [] }).ok).toBe(false);
  });

  it("property: any accepted URL is https, has no userinfo, and its host is on the allowlist with an allowed port", () => {
    const host = fc.oneof(
      fc.constantFrom("seller.example.org", "SELLER.EXAMPLE.ORG", "localhost", "127.0.0.1", "[::1]", "10.1.2.3", "evil.org", "seller.example.org."),
      fc.domain(),
      fc.ipV4().map(String),
    );
    const scheme = fc.constantFrom("https://", "http://", "HTTPS://", "ftp://", "");
    const userinfo = fc.constantFrom("", "u@", "u:p@");
    const port = fc.constantFrom("", ":443", ":8443", ":8080", ":0");
    fc.assert(
      fc.property(scheme, userinfo, host, port, fc.webPath(), (s, ui, h, p, path) => {
        const r = checkEndpoint(`${s}${ui}${h}${p}${path}`, policy);
        if (!r.ok) return;
        const u = new URL(r.url);
        expect(u.protocol).toBe("https:");
        expect(u.username + u.password).toBe("");
        expect(u.hostname).toBe("seller.example.org");
        expect(["", "8443"]).toContain(u.port);
      }),
      { numRuns: 2000 },
    );
  });

  it("property: arbitrary IPv4 literals are never accepted, even if allowlisted", () => {
    fc.assert(
      fc.property(fc.ipV4(), (ip) => {
        expect(checkEndpoint(`https://${ip}/x`, { allowedHosts: [ip] }).ok).toBe(false);
      }),
    );
  });
});
