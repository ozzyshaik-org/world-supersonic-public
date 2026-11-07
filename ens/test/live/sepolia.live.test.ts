// Evidence tier: live-testnet-read. Real eth_call against public Sepolia RPC, no writes, no keys.
// Skipped unless ENS_LIVE=1 (so CI and offline runs do not depend on a public RPC).
// Expected values were verified independently on 2026-11-07 (IST) with a hand-written Python script that
// ABI-encodes UniversalResolver.resolve(bytes,bytes) and sends a raw eth_call to
// 0xeEeEEEeE14D718C2B47D9923Deab1335E144EeEe on Sepolia (no viem, no ENS library). See ens/README.md.
// Note: gregskril.eth is served via the ENSv2 Universal Resolver, but the resolver it reported
// (0x322b7581...89cb0) is the ENSv1Resolver listed in the Sepolia (ENSv2) deployments table, i.e. an
// unmigrated v1 name read through the v2 entry point, not a name stored in a v2 Permissioned Resolver.
import { describe, expect, it } from "vitest";
import { resolveEns } from "../../src/index.js";

const live = process.env.ENS_LIVE === "1" ? describe : describe.skip;

live("ENSv2 Universal Resolver on Sepolia (live-testnet-read)", () => {
  it("resolves gregskril.eth addr and avatar text, matching the independent raw eth_call", async () => {
    const r = await resolveEns("gregskril.eth", ["avatar", "com.twitter"]);
    expect(r.chainId).toBe(11155111);
    expect(r.address).toBe("0x179A862703a4adfb29896552DF9e307980D19285");
    expect(r.texts).toEqual({ avatar: "https://gregskril.com/img/profile.jpg", "com.twitter": null });
  }, 30_000);

  it("returns null address for a name with no addr record (ens.eth on Sepolia, checked by raw eth_call)", async () => {
    const r = await resolveEns("ens.eth", []);
    expect(r.address).toBeNull();
  }, 30_000);
});
