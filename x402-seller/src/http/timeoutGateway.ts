/**
 * Wraps a GatewayClient so settle() is aborted after settleTimeoutMs.
 * Lookups are left unwrapped (reconciliation can use its own budgets later).
 */

import type { GatewayClient } from "../gateway/client.js";
import type {
  PaymentPayload,
  PaymentRequirements,
  SettleResponse,
  TransferRecord,
  TransferStatus,
} from "../types.js";

export class TimeoutGatewayClient implements GatewayClient {
  readonly name: string;
  readonly inner: GatewayClient;
  private readonly settleTimeoutMs: number;

  constructor(inner: GatewayClient, settleTimeoutMs: number) {
    this.inner = inner;
    this.name = `Timeout(${inner.name})`;
    this.settleTimeoutMs = settleTimeoutMs;
  }

  async settle(
    payload: PaymentPayload,
    requirements: PaymentRequirements,
  ): Promise<SettleResponse> {
    return withTimeout(
      this.inner.settle(payload, requirements),
      this.settleTimeoutMs,
      "settle_timeout",
    );
  }

  getTransferById(id: string): Promise<TransferRecord | null> {
    return this.inner.getTransferById(id);
  }

  searchTransfers(params: {
    nonce?: string;
    from?: string;
    to?: string;
    network: string;
    status?: TransferStatus;
  }): Promise<TransferRecord[]> {
    return this.inner.searchTransfers(params);
  }
}

function withTimeout<T>(p: Promise<T>, ms: number, label: string): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    const t = setTimeout(() => {
      const err = new Error(label);
      (err as Error & { retryable: boolean }).retryable = true;
      reject(err);
    }, ms);
    p.then(
      (v) => {
        clearTimeout(t);
        resolve(v);
      },
      (e) => {
        clearTimeout(t);
        reject(e);
      },
    );
  });
}
