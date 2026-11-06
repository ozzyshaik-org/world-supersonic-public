/**
 * FAKE Gateway — synthetic fixture only. Not live Arc Testnet / Circle Gateway.
 * All transfer ids and responses are labelled so they can never be mistaken for evidence.
 */

import type { GatewayClient } from "./client.js";
import type {
  PaymentPayload,
  PaymentRequirements,
  SettleResponse,
  TransferRecord,
  TransferStatus,
} from "../types.js";

export type FakeSettleMode =
  | { kind: "success"; transferId?: string }
  | { kind: "success_missing_transaction" }
  | { kind: "reject"; errorReason: string }
  | { kind: "reject_missing_errorReason" }
  | { kind: "http400"; errorReason?: string }
  | { kind: "http500" }
  | { kind: "malformed" }
  | { kind: "uncertain"; reason: "timeout" | "empty_body" | "unexpected_error" | "drop" }
  | { kind: "nonce_already_used" };

export interface FakeGatewayOptions {
  settleMode?: FakeSettleMode;
  /** Pre-seeded transfers keyed by nonce (lowercase). */
  transfersByNonce?: Map<string, TransferRecord>;
  /** Pre-seeded transfers keyed by id. */
  transfersById?: Map<string, TransferRecord>;
  /** When true, getTransferById / searchTransfers throw. */
  lookupFails?: boolean;
  /** Extra transfers returned by search to force ambiguity. */
  ambiguousExtra?: TransferRecord[];
}

let fakeSeq = 0;

function syntheticTransferId(): string {
  fakeSeq += 1;
  return `fake-transfer-${fakeSeq.toString().padStart(4, "0")}`;
}

export class FakeGatewayClient implements GatewayClient {
  readonly name = "FAKE_GatewayClient";

  settleCalls: Array<{ payload: PaymentPayload; requirements: PaymentRequirements }> = [];
  settleMode: FakeSettleMode;
  transfersByNonce: Map<string, TransferRecord>;
  transfersById: Map<string, TransferRecord>;
  settleModeOnce: FakeSettleMode | null = null;
  lookupFails: boolean;
  ambiguousExtra: TransferRecord[];
  getTransferByIdCalls = 0;
  searchTransfersCalls = 0;

  constructor(options: FakeGatewayOptions = {}) {
    this.settleMode = options.settleMode ?? { kind: "success" };
    this.transfersByNonce = options.transfersByNonce ?? new Map();
    this.transfersById = options.transfersById ?? new Map();
    this.lookupFails = options.lookupFails ?? false;
    this.ambiguousExtra = options.ambiguousExtra ?? [];
  }

  seedTransfer(record: TransferRecord): void {
    this.transfersByNonce.set(record.nonce.toLowerCase(), record);
    this.transfersById.set(record.id, record);
  }

  /** Seed without overwriting nonce map uniqueness — for ambiguity tests. */
  seedExtraTransfer(record: TransferRecord): void {
    this.transfersById.set(record.id, record);
    this.ambiguousExtra.push(record);
  }

  async settle(
    payload: PaymentPayload,
    requirements: PaymentRequirements,
  ): Promise<SettleResponse> {
    this.settleCalls.push({ payload, requirements });
    const mode = this.settleModeOnce ?? this.settleMode;
    this.settleModeOnce = null;

    switch (mode.kind) {
      case "success": {
        const auth = payload.payload.authorization;
        const id = mode.transferId ?? syntheticTransferId();
        const record: TransferRecord = {
          id,
          status: "received",
          token: "USDC",
          sendingNetwork: requirements.network,
          recipientNetwork: requirements.network,
          fromAddress: auth.from.toLowerCase(),
          toAddress: auth.to.toLowerCase(),
          amount: String(auth.value),
          nonce: auth.nonce.toLowerCase(),
          txHash: null,
          createdAt: "1970-01-01T00:00:00.000Z",
          updatedAt: "1970-01-01T00:00:00.000Z",
        };
        this.seedTransfer(record);
        return {
          success: true,
          payer: auth.from.toLowerCase(),
          transaction: id,
        };
      }
      case "success_missing_transaction":
        return {
          success: true,
          payer: "0xfake",
          transaction: "",
        };
      case "reject":
        return {
          success: false,
          errorReason: mode.errorReason,
          transaction: "",
        };
      case "reject_missing_errorReason":
        return {
          success: false,
          transaction: "",
        };
      case "http400": {
        const err = new Error(
          `FAKE Gateway HTTP 400: ${mode.errorReason ?? "invalid_payload"}`,
        );
        (err as Error & { httpStatus: number }).httpStatus = 400;
        (err as Error & { errorReason: string }).errorReason =
          mode.errorReason ?? "invalid_payload";
        throw err;
      }
      case "http500": {
        const err = new Error("FAKE Gateway HTTP 500 unexpected_error");
        (err as Error & { httpStatus: number }).httpStatus = 500;
        throw err;
      }
      case "malformed":
        return { success: true, transaction: "" } as SettleResponse;
      case "uncertain": {
        if (mode.reason === "empty_body") {
          const err = new Error("Circle Gateway settle returned empty response (FAKE/500)");
          (err as Error & { httpStatus: number }).httpStatus = 500;
          throw err;
        }
        if (mode.reason === "unexpected_error") {
          return {
            success: false,
            errorReason: "unexpected_error",
            transaction: "",
          };
        }
        const err = new Error(`FAKE Gateway settle ${mode.reason}`);
        (err as Error & { uncertain: boolean }).uncertain = true;
        throw err;
      }
      case "nonce_already_used":
        return {
          success: false,
          errorReason: "nonce_already_used",
          transaction: "",
        };
      default: {
        const _exhaustive: never = mode;
        return _exhaustive;
      }
    }
  }

  async getTransferById(id: string): Promise<TransferRecord | null> {
    this.getTransferByIdCalls += 1;
    if (this.lookupFails) {
      throw new Error("FAKE Gateway lookup failure (getTransferById)");
    }
    return this.transfersById.get(id) ?? null;
  }

  async searchTransfers(params: {
    nonce?: string;
    from?: string;
    to?: string;
    network: string;
    status?: TransferStatus;
  }): Promise<TransferRecord[]> {
    this.searchTransfersCalls += 1;
    if (this.lookupFails) {
      throw new Error("FAKE Gateway lookup failure (searchTransfers)");
    }
    if (!params.network) {
      throw new Error("FAKE Gateway: network must be passed explicitly");
    }
    let results = [...this.transfersByNonce.values()].filter(
      (t) => t.sendingNetwork === params.network,
    );
    // Include ambiguous extras that match filters
    for (const extra of this.ambiguousExtra) {
      if (extra.sendingNetwork === params.network) {
        results.push(extra);
      }
    }
    if (params.nonce) {
      const n = params.nonce.toLowerCase();
      results = results.filter((t) => t.nonce.toLowerCase() === n);
    }
    if (params.from) {
      const f = params.from.toLowerCase();
      results = results.filter((t) => t.fromAddress.toLowerCase() === f);
    }
    if (params.to) {
      const to = params.to.toLowerCase();
      results = results.filter((t) => t.toAddress.toLowerCase() === to);
    }
    if (params.status) {
      results = results.filter((t) => t.status === params.status);
    }
    return results;
  }
}

/** Build an obviously synthetic transfer fixture (never use as live evidence). */
export function makeFakeTransfer(
  partial: Partial<TransferRecord> &
    Pick<TransferRecord, "nonce" | "fromAddress" | "toAddress" | "amount" | "sendingNetwork">,
): TransferRecord {
  const id = partial.id ?? `fake-transfer-fixture-${partial.nonce.slice(0, 10)}`;
  return {
    id,
    status: partial.status ?? "received",
    token: "USDC",
    sendingNetwork: partial.sendingNetwork,
    recipientNetwork: partial.recipientNetwork ?? partial.sendingNetwork,
    fromAddress: partial.fromAddress.toLowerCase(),
    toAddress: partial.toAddress.toLowerCase(),
    amount: partial.amount,
    nonce: partial.nonce.toLowerCase(),
    txHash: partial.txHash ?? null,
    createdAt: partial.createdAt ?? "1970-01-01T00:00:00.000Z",
    updatedAt: partial.updatedAt ?? "1970-01-01T00:00:00.000Z",
  };
}
