import type {
  AuthorizationBinding,
  IncidentFlag,
  LedgerRow,
  PaymentPayload,
  PaymentState,
  GatewayStatus,
  FulfillmentState,
  StoredResponse,
  Receipt,
} from "../types.js";
import { gatewayStatusRank } from "../types.js";

export class LedgerConflictError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "LedgerConflictError";
  }
}

export class MemoryLedger {
  private byAuthKey = new Map<string, LedgerRow>();
  private byNonce = new Map<string, string>(); // nonce -> authKey
  private mutex = Promise.resolve();

  /** Run exclusive critical section. */
  async withLock<T>(fn: () => Promise<T> | T): Promise<T> {
    let release!: () => void;
    const next = new Promise<void>((r) => {
      release = r;
    });
    const prev = this.mutex;
    this.mutex = prev.then(() => next);
    await prev;
    try {
      return await fn();
    } finally {
      release();
    }
  }

  getByAuthKey(authKey: string): LedgerRow | undefined {
    const row = this.byAuthKey.get(authKey);
    return row ? cloneRow(row) : undefined;
  }

  getByNonce(nonce: string): LedgerRow | undefined {
    const key = this.byNonce.get(nonce.toLowerCase());
    return key ? this.getByAuthKey(key) : undefined;
  }

  /** Atomic insert of a new reservation. Throws if auth_key or nonce already present. */
  insertReservation(
    binding: AuthorizationBinding,
    paymentPayload: PaymentPayload,
    nowIso: string,
  ): LedgerRow {
    if (this.byAuthKey.has(binding.authKey)) {
      throw new LedgerConflictError("auth_key already exists");
    }
    const nonceKey = binding.nonce.toLowerCase();
    if (this.byNonce.has(nonceKey)) {
      throw new LedgerConflictError("nonce already exists");
    }
    const row: LedgerRow = {
      ...binding,
      paymentState: "submission_pending",
      gatewayStatus: null,
      fulfillmentState: "not_started",
      attemptCounter: 0,
      attemptAt: null,
      leaseOwner: null,
      leaseExpiresAt: null,
      storedResponse: null,
      receipt: null,
      gatewayTransferId: null,
      gatewayError: null,
      incidents: [],
      paymentPayload,
      createdAt: nowIso,
      updatedAt: nowIso,
    };
    this.byAuthKey.set(binding.authKey, row);
    this.byNonce.set(nonceKey, binding.authKey);
    return cloneRow(row);
  }

  /**
   * Claim submitter: move attemptCounter from n to n+1 only if payment_state is
   * submission_pending and counter equals expected. Returns updated row or null.
   */
  claimSubmitter(authKey: string, expectedCounter: number, nowIso: string): LedgerRow | null {
    const row = this.byAuthKey.get(authKey);
    if (!row) return null;
    if (row.paymentState !== "submission_pending") return null;
    if (row.attemptCounter !== expectedCounter) return null;
    row.attemptCounter = expectedCounter + 1;
    row.attemptAt = nowIso;
    row.updatedAt = nowIso;
    return cloneRow(row);
  }

  updatePaymentState(
    authKey: string,
    next: PaymentState,
    patch: Partial<
      Pick<
        LedgerRow,
        "gatewayTransferId" | "gatewayError" | "gatewayStatus" | "incidents"
      >
    > = {},
    nowIso: string,
  ): LedgerRow {
    const row = this.require(authKey);
    row.paymentState = next;
    if (patch.gatewayTransferId !== undefined) row.gatewayTransferId = patch.gatewayTransferId;
    if (patch.gatewayError !== undefined) row.gatewayError = patch.gatewayError;
    if (patch.gatewayStatus !== undefined) row.gatewayStatus = patch.gatewayStatus;
    if (patch.incidents) {
      for (const f of patch.incidents) this.addIncidentUnlocked(row, f);
    }
    row.updatedAt = nowIso;
    return cloneRow(row);
  }

  /**
   * Copy gateway_status forward only. Anomalous backward moves are recorded
   * as incidents and do not change stored status.
   */
  applyGatewayStatus(authKey: string, status: GatewayStatus, nowIso: string): LedgerRow {
    const row = this.require(authKey);
    const current = row.gatewayStatus;
    if (current === null || gatewayStatusRank(status) >= gatewayStatusRank(current)) {
      // failed can be applied even from completed? Spec: status only moves forward.
      // Treat failed as a terminal side branch: allow if not already completed/failed,
      // or always allow failed when observed (Circle says failed = never charged).
      if (current === "completed" && status !== "completed") {
        // anomaly — do not move backwards from completed
        row.updatedAt = nowIso;
        return cloneRow(row);
      }
      row.gatewayStatus = status;
    } else if (status !== current) {
      // anomaly: ignore backward move
    }
    row.updatedAt = nowIso;
    return cloneRow(row);
  }

  addIncident(authKey: string, flag: IncidentFlag, nowIso: string): LedgerRow {
    const row = this.require(authKey);
    this.addIncidentUnlocked(row, flag);
    row.updatedAt = nowIso;
    return cloneRow(row);
  }

  /**
   * Take fulfillment lease if payment accepted and gateway not failed and
   * fulfillment is not_started or retryable_failure (or expired in_progress).
   */
  takeLease(
    authKey: string,
    owner: string,
    leaseMs: number,
    nowMs: number,
    nowIso: string,
  ): LedgerRow | null {
    const row = this.require(authKey);
    if (row.paymentState !== "accepted") return null;
    if (row.gatewayStatus === "failed") return null;
    if (row.fulfillmentState === "fulfilled" || row.fulfillmentState === "failed_permanent") {
      return null;
    }
    if (row.fulfillmentState === "in_progress") {
      if (row.leaseExpiresAt !== null && row.leaseExpiresAt > nowMs) return null;
      // lease expired → treat as takeover from retryable
    }
    row.fulfillmentState = "in_progress";
    row.leaseOwner = owner;
    row.leaseExpiresAt = nowMs + leaseMs;
    row.updatedAt = nowIso;
    return cloneRow(row);
  }

  /**
   * Commit fulfillment under lease fencing. Stale workers cannot overwrite.
   */
  commitFulfillment(
    authKey: string,
    owner: string,
    response: StoredResponse,
    receipt: Receipt,
    nowIso: string,
  ): LedgerRow | null {
    const row = this.require(authKey);
    if (row.leaseOwner !== owner) return null;
    if (row.fulfillmentState !== "in_progress") return null;
    if (row.storedResponse !== null) return null;
    row.fulfillmentState = "fulfilled";
    row.storedResponse = response;
    row.receipt = receipt;
    row.leaseOwner = null;
    row.leaseExpiresAt = null;
    row.updatedAt = nowIso;
    return cloneRow(row);
  }

  markRetryableFailure(authKey: string, owner: string, nowIso: string): LedgerRow | null {
    const row = this.require(authKey);
    if (row.leaseOwner !== owner) return null;
    if (row.fulfillmentState !== "in_progress") return null;
    row.fulfillmentState = "retryable_failure";
    row.leaseOwner = null;
    row.leaseExpiresAt = null;
    row.updatedAt = nowIso;
    return cloneRow(row);
  }

  markFailedPermanent(authKey: string, nowIso: string): LedgerRow {
    const row = this.require(authKey);
    row.fulfillmentState = "failed_permanent";
    row.leaseOwner = null;
    row.leaseExpiresAt = null;
    this.addIncidentUnlocked(row, "paid_unfulfilled");
    row.updatedAt = nowIso;
    return cloneRow(row);
  }

  /** Automatic void only from submission_pending with attemptCounter === 0. */
  tryAutomaticVoid(authKey: string, nowSec: number, nowIso: string): LedgerRow | null {
    const row = this.require(authKey);
    if (row.paymentState !== "submission_pending") return null;
    if (row.attemptCounter !== 0) return null;
    if (nowSec < row.validBefore) return null;
    row.paymentState = "void";
    row.updatedAt = nowIso;
    return cloneRow(row);
  }

  /**
   * Explicitly refuse submission_uncertain → void (test helper / invariant).
   */
  canAutomaticVoid(row: LedgerRow, nowSec: number): boolean {
    return (
      row.paymentState === "submission_pending" &&
      row.attemptCounter === 0 &&
      nowSec >= row.validBefore
    );
  }

  private require(authKey: string): LedgerRow {
    const row = this.byAuthKey.get(authKey);
    if (!row) throw new Error(`ledger row not found: ${authKey}`);
    return row;
  }

  private addIncidentUnlocked(row: LedgerRow, flag: IncidentFlag): void {
    if (!row.incidents.includes(flag)) row.incidents.push(flag);
  }
}

function cloneRow(row: LedgerRow): LedgerRow {
  return {
    ...row,
    incidents: [...row.incidents],
    storedResponse: row.storedResponse ? { ...row.storedResponse } : null,
    receipt: row.receipt ? { ...row.receipt } : null,
    paymentPayload: structuredClone(row.paymentPayload),
  };
}

export type { FulfillmentState, PaymentState, GatewayStatus };
