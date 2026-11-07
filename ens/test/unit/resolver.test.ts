import { describe, expect, it } from "vitest";
import fc from "fast-check";
import { EnsInputError, MAX_TEXT_KEYS, normalizeName, resolveEns, sepoliaClient } from "../../src/index.js";
import { fakeTransport, UR_PROXY } from "./fakeRpc.js";

// Independent oracles:
//  - EIP-137 published namehash vectors (https://eips.ethereum.org/EIPS/eip-137#namehash-algorithm):
//      namehash('eth')     = 0x93cdeb708b7545dc668eb9280176169d1c33cfd8ed6f04690a0bcc88a93fc4ae
//      namehash('foo.eth') = 0xde9b09fd7c5f901e23a3f19fecc54828e9c848539801e86591bd9801b019f84f
//  - DNS wire encoding (ENSIP-10) of foo.eth written by hand: 03 'f''o''o' 03 'e''t''h' 00
const FOO_ETH_NODE = "0xde9b09fd7c5f901e23a3f19fecc54828e9c848539801e86591bd9801b019f84f";
const FOO_ETH_DNS = "0x03666f6f0365746800";
const ADDR = "0x179A862703a4adfb29896552DF9e307980D19285";

function setup() {
  const f = fakeTransport({
    addr: { [FOO_ETH_NODE]: ADDR },
    text: { [FOO_ETH_NODE]: { url: "https://example.org/x402", description: "seller" } },
  });
  return { ...f, client: sepoliaClient(f.transport) };
}

describe("resolveEns (unit, fake transport)", () => {
  it("queries the Universal Resolver proxy with the EIP-137 namehash and DNS-encoded name, returns the addr", async () => {
    const { client, seen } = setup();
    const r = await resolveEns("foo.eth", [], client);
    expect(r).toEqual({ inputName: "foo.eth", name: "foo.eth", address: ADDR, texts: {}, chainId: 11155111 });
    expect(seen).toHaveLength(1);
    expect(seen[0]).toMatchObject({ to: UR_PROXY, dnsName: FOO_ETH_DNS, node: FOO_ETH_NODE, fn: "addr" });
  });

  it("normalises mixed case to the same node before resolving (ENSIP-15)", async () => {
    const { client, seen } = setup();
    const r = await resolveEns("Foo.ETH", [], client);
    expect(r.name).toBe("foo.eth");
    expect(r.inputName).toBe("Foo.ETH");
    expect(seen[0]!.node).toBe(FOO_ETH_NODE);
  });

  it("returns requested text records, with null for unset keys, and asks only for those keys", async () => {
    const { client, seen } = setup();
    const r = await resolveEns("foo.eth", ["url", "com.github", "url"], client);
    expect(r.texts).toEqual({ url: "https://example.org/x402", "com.github": null });
    expect(seen.filter((s) => s.fn === "text").map((s) => s.key).sort()).toEqual(["com.github", "url"]);
  });

  it("returns address null for a name with no addr record", async () => {
    const { client } = setup();
    const r = await resolveEns("bar.eth", ["url"], client);
    expect(r.address).toBeNull();
    expect(r.texts).toEqual({ url: null });
  });

  it.each(["", "eth", "foo..eth", "foo bar.eth", ".eth", "foo.eth."])(
    "rejects malformed name %j before any RPC call",
    async (bad) => {
      const { client, seen } = setup();
      await expect(resolveEns(bad, [], client)).rejects.toMatchObject({ code: "MALFORMED_NAME" });
      expect(seen).toHaveLength(0);
    },
  );

  it.each([["has space"], [""], ["x".repeat(65)], ["tab\tkey"]])("rejects bad text key %j before any RPC call", async (k) => {
    const { client, seen } = setup();
    await expect(resolveEns("foo.eth", [k], client)).rejects.toBeInstanceOf(EnsInputError);
    expect(seen).toHaveLength(0);
  });

  it("caps the number of text keys", async () => {
    const { client, seen } = setup();
    const keys = Array.from({ length: MAX_TEXT_KEYS + 1 }, (_, i) => `k${i}`);
    await expect(resolveEns("foo.eth", keys, client)).rejects.toMatchObject({ code: "BAD_TEXT_KEY" });
    expect(seen).toHaveLength(0);
  });
});

describe("normalizeName (property)", () => {
  it("is case-insensitive for ASCII labels and idempotent", () => {
    const label = fc.stringMatching(/^[a-z0-9]{1,12}$/);
    fc.assert(
      fc.property(label, label, (a, b) => {
        const lower = `${a}.${b}`;
        const mixed = `${a.toUpperCase()}.${b}`;
        expect(normalizeName(mixed)).toBe(lower);
        expect(normalizeName(normalizeName(mixed))).toBe(lower);
      }),
    );
  });
});
