import { describe, expect, it } from "vitest";
import { CircleBatchFacilitatorAdapter } from "../src/gateway/circleAdapter.js";
import type { PaymentPayload, PaymentRequirements, SettleResponse } from "../src/types.js";
import { ARC_TESTNET_GATEWAY_WALLET } from "../src/chainConfig.js";
import { makePayment } from "./helpers.js";

describe("Circle adapter import/contract (no network)", () => {
  it("imports @circle-fin/x402-batching/server without constructing a live client", async () => {
    const mod = await import("@circle-fin/x402-batching/server");
    expect(mod).toBeTypeOf("object");
    expect(typeof mod.BatchFacilitatorClient).toBe("function");
    // Do NOT construct: new BatchFacilitatorClient() would be a live path.
  });

  it("adapter settles only through the injected facilitator (no network)", async () => {
    let called = 0;
    const facilitator = {
      async settle(
        _payload: PaymentPayload,
        _requirements: PaymentRequirements,
      ): Promise<SettleResponse> {
        called += 1;
        return { success: true, transaction: "fake-injected-tx", payer: "0xfake" };
      },
    };
    const transfers = {
      async getTransferById() {
        return null;
      },
      async searchTransfers() {
        return [];
      },
    };
    const adapter = new CircleBatchFacilitatorAdapter({ facilitator, transfers });
    const requirements: PaymentRequirements = {
      scheme: "exact",
      network: "eip155:5042002",
      asset: "0x3600000000000000000000000000000000000000",
      amount: "10000",
      payTo: "0x1111111111111111111111111111111111111111",
      maxTimeoutSeconds: 345600,
      extra: {
        name: "GatewayWalletBatched",
        version: "1",
        verifyingContract: ARC_TESTNET_GATEWAY_WALLET,
      },
    };
    const result = await adapter.settle(makePayment(), requirements);
    expect(called).toBe(1);
    expect(result.transaction).toBe("fake-injected-tx");
  });
});
