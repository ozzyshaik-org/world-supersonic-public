/**
 * Optional live adapter wrapping @circle-fin/x402-batching BatchFacilitatorClient.
 * Not constructed or called in the first checkpoint (no credentials / no live calls).
 */

import type { GatewayClient } from "./client.js";
import type {
  PaymentPayload,
  PaymentRequirements,
  SettleResponse,
  TransferRecord,
  TransferStatus,
} from "../types.js";

export interface CircleAdapterDeps {
  /**
   * Injected BatchFacilitatorClient-like object. Kept untyped against the package
   * so unit tests never need the optional dependency loaded.
   */
  facilitator: {
    settle(
      payload: PaymentPayload,
      requirements: PaymentRequirements,
    ): Promise<SettleResponse>;
  };
  /** Transfer lookup surface (GatewayClient buyer APIs or REST). */
  transfers: {
    getTransferById(id: string): Promise<TransferRecord | null>;
    searchTransfers(params: {
      nonce?: string;
      from?: string;
      to?: string;
      network: string;
      status?: TransferStatus;
    }): Promise<TransferRecord[]>;
  };
}

export class CircleBatchFacilitatorAdapter implements GatewayClient {
  readonly name = "CircleBatchFacilitatorAdapter";
  private readonly deps: CircleAdapterDeps;

  constructor(deps: CircleAdapterDeps) {
    this.deps = deps;
  }

  settle(
    payload: PaymentPayload,
    requirements: PaymentRequirements,
  ): Promise<SettleResponse> {
    return this.deps.facilitator.settle(payload, requirements);
  }

  getTransferById(id: string): Promise<TransferRecord | null> {
    return this.deps.transfers.getTransferById(id);
  }

  searchTransfers(params: {
    nonce?: string;
    from?: string;
    to?: string;
    network: string;
    status?: TransferStatus;
  }): Promise<TransferRecord[]> {
    if (!params.network) {
      throw new Error("searchTransfers requires an explicit network");
    }
    return this.deps.transfers.searchTransfers(params);
  }
}
