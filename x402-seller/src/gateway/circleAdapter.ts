/**
 * Injected-interface wrapper around @circle-fin/x402-batching BatchFacilitatorClient.
 *
 * This adapter is NOT a verified live integration. It does not construct a live
 * client and makes no network calls. Unit tests inject fakes.
 *
 * Import compatibility: the official server entry requires peer @x402/evm
 * (declared in this package). See docs/CIRCLE_RECHECK.md.
 */

import type { GatewayClient } from "./client.js";
import type {
  PaymentPayload,
  PaymentRequirements,
  SettleResponse,
  TransferRecord,
  TransferStatus,
} from "../types.js";

/**
 * Minimal surface of BatchFacilitatorClient.settle used by this adapter.
 * Kept structurally compatible with @circle-fin/x402-batching/server types.
 */
export interface CircleFacilitatorLike {
  settle(
    payload: PaymentPayload,
    requirements: PaymentRequirements,
  ): Promise<SettleResponse>;
}

export interface CircleTransferLookup {
  getTransferById(id: string): Promise<TransferRecord | null>;
  searchTransfers(params: {
    nonce?: string;
    from?: string;
    to?: string;
    network: string;
    status?: TransferStatus;
  }): Promise<TransferRecord[]>;
}

export interface CircleAdapterDeps {
  facilitator: CircleFacilitatorLike;
  transfers: CircleTransferLookup;
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

/**
 * Type-only / import-contract helper. Callers that want to construct a live
 * BatchFacilitatorClient should import from @circle-fin/x402-batching/server
 * themselves after installing peers. This re-export path is exercised by the
 * no-network import test without constructing a client.
 */
export type { GatewayClient };
