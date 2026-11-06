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
import { gatewayStatusRank, isKnownGatewayStatus } from "../types.js";

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

  /** Run exclusive critical section. Callers must NOT hold this across network I/O. */
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
      leaseGeneration: 0,
      leaseExpiresAt: null,
      fulfillmentAttemptCount: 0,
      fulfillmentFirstAttemptAt: null,
      storedResponse: null,
      receipt: null,
      gatewayTransferId: null,
      gatewayError: null,
      incidents: [],
      anomalies: [],
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
   * Copy gateway_status forward only. Unknown or backward statuses do NOT mutate
   * gateway_status; they are recorded as operator-visible anomalies.
   */
  applyGatewayStatus(authKey: string, status: unknown, nowIso: string): LedgerRow {
    const row = this.require(authKey);
    if (!isKnownGatewayStatus(status)) {
      this.recordAnomalyUnlocked(
        row,
        `unknown_gateway_status:${String(status)}`,
      );
      row.updatedAt = nowIso;
      return cloneRow(row);
    }
    const current = row.gatewayStatus;
    if (current === null) {
      row.gatewayStatus = status;
      row.updatedAt = nowIso;
      return cloneRow(row);
    }
    // Never move backwards from completed.
    if (current === "completed" && status !== "completed") {
      this.recordAnomalyUnlocked(
        row,
        `backward_gateway_status:${current}->${status}`,
      );
      row.updatedAt = nowIso;
      return cloneRow(row);
    }
    // failed is a terminal side-branch: allow from non-completed/non-failed.
    if (status === "failed") {
      if (current !== "failed") {
        row.gatewayStatus = "failed";
      }
      row.updatedAt = nowIso;
      return cloneRow(row);
    }
    if (gatewayStatusRank(status) >= gatewayStatusRank(current)) {
      row.gatewayStatus = status;
    } else if (status !== current) {
      this.recordAnomalyUnlocked(
        row,
        `backward_gateway_status:${current}->${status}`,
      );
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

  recordAnomaly(authKey: string, message: string, nowIso: string): LedgerRow {
    const row = this.require(authKey);
    this.recordAnomalyUnlocked(row, message);
    row.updatedAt = nowIso;
    return cloneRow(row);
  }

  /**
   * Take fulfillment lease if payment accepted and gateway not failed and
   * fulfillment is not_started or retryable_failure (or expired in_progress).
   * Owner must be a cryptographically unique identity; generation is incremented.
   */
  takeLease(
    authKey: string,
    owner: string,
    leaseMs: number,
    nowMs: number,
    nowIso: string,
  ): { row: LedgerRow; generation: number } | null {
    const row = this.require(authKey);
    if (row.paymentState !== "accepted") return null;
    if (row.gatewayStatus === "failed") return null;
    if (row.fulfillmentState === "fulfilled" || row.fulfillmentState === "failed_permanent") {
      return null;
    }
    if (row.fulfillmentState === "in_progress") {
      if (row.leaseExpiresAt !== null && row.leaseExpiresAt > nowMs) return null;
      // lease expired → takeover
    }
    row.fulfillmentState = "in_progress";
    row.leaseOwner = owner;
    row.leaseGeneration += 1;
    row.leaseExpiresAt = nowMs + leaseMs;
    if (row.fulfillmentAttemptCount === 0) {
      row.fulfillmentFirstAttemptAt = nowIso;
    }
    row.fulfillmentAttemptCount += 1;
    row.updatedAt = nowIso;
    return { row: cloneRow(row), generation: row.leaseGeneration };
  }

  /**
   * Commit fulfillment under lease fencing. Checks owner, generation, and that
   * the lease has not expired. Stale / expired workers cannot overwrite.
   */
  commitFulfillment(
    authKey: string,
    owner: string,
    generation: number,
    nowMs: number,
    response: StoredResponse,
    receipt: Receipt,
    nowIso: string,
  ): LedgerRow | null {
    const row = this.require(authKey);
    if (row.leaseOwner !== owner) return null;
    if (row.leaseGeneration !== generation) return null;
    if (row.fulfillmentState !== "in_progress") return null;
    if (row.leaseExpiresAt !== null && row.leaseExpiresAt <= nowMs) return null;
    if (row.storedResponse !== null) return null;
    // Only 2xx may be committed by the engine; ledger still accepts the stored
    // response object but callers must enforce the 2xx rule before calling.
    row.fulfillmentState = "fulfilled";
    row.storedResponse = response;
    row.receipt = receipt;
    row.leaseOwner = null;
    row.leaseExpiresAt = null;
    row.updatedAt = nowIso;
    return cloneRow(row);
  }

  markRetryableFailure(
    authKey: string,
    owner: string,
    generation: number,
    nowMs: number,
    nowIso: string,
  ): LedgerRow | null {
    const row = this.require(authKey);
    if (!this.leaseFenceHolds(row, owner, generation, nowMs)) return null;
    row.fulfillmentState = "retryable_failure";
    row.leaseOwner = null;
    row.leaseExpiresAt = null;
    row.updatedAt = nowIso;
    return cloneRow(row);
  }

  /**
   * Lease-fenced permanent failure. Fence is REQUIRED: owner+generation+unexpired
   * in_progress. There is no unfenced path — releasing via markRetryableFailure
   * and later calling markFailedPermanent without a live lease was the confirmed
   * race that could overwrite a newer worker's in_progress attempt.
   */
  markFailedPermanent(
    authKey: string,
    nowIso: string,
    fence?: { owner: string; generation: number; nowMs: number },
  ): LedgerRow | null {
    // Unfenced calls are a hard no-op. The pre-fix budget path released via
    // markRetryableFailure then called markFailedPermanent without a fence,
    // which could overwrite a newer in_progress lease. Callers must use
    // resolveRetryableUnderLease for budget decisions under the live lease.
    if (!fence) return null;
    const row = this.require(authKey);
    if (!this.leaseFenceHolds(row, fence.owner, fence.generation, fence.nowMs)) {
      return null;
    }
    row.fulfillmentState = "failed_permanent";
    row.leaseOwner = null;
    row.leaseExpiresAt = null;
    this.addIncidentUnlocked(row, "paid_unfulfilled");
    row.updatedAt = nowIso;
    return cloneRow(row);
  }

  /**
   * Single atomic CAS under the ORIGINAL lease: either record retryable_failure
   * and release, or record failed_permanent (budget exhausted). Never release
   * then re-decide under a fence the caller already dropped.
   */
  resolveRetryableUnderLease(
    authKey: string,
    owner: string,
    generation: number,
    nowMs: number,
    nowIso: string,
    budget: { maxAttempts: number; maxMs: number; nowBusinessMs: number },
  ): { row: LedgerRow; exhausted: boolean } | null {
    const row = this.require(authKey);
    if (!this.leaseFenceHolds(row, owner, generation, nowMs)) return null;

    let timeExceeded = false;
    if (row.fulfillmentFirstAttemptAt) {
      const first = Date.parse(row.fulfillmentFirstAttemptAt);
      if (Number.isFinite(first)) {
        timeExceeded = budget.nowBusinessMs - first >= budget.maxMs;
      }
    }
    const countExceeded = row.fulfillmentAttemptCount >= budget.maxAttempts;
    const exhausted = countExceeded || timeExceeded;

    if (exhausted) {
      row.fulfillmentState = "failed_permanent";
      this.addIncidentUnlocked(row, "paid_unfulfilled");
    } else {
      row.fulfillmentState = "retryable_failure";
    }
    row.leaseOwner = null;
    row.leaseExpiresAt = null;
    row.updatedAt = nowIso;
    return { row: cloneRow(row), exhausted };
  }

  /** Shared fence check: owner, generation, in_progress, unexpired. */
  private leaseFenceHolds(
    row: LedgerRow,
    owner: string,
    generation: number,
    nowMs: number,
  ): boolean {
    if (row.leaseOwner !== owner) return false;
    if (row.leaseGeneration !== generation) return false;
    if (row.fulfillmentState !== "in_progress") return false;
    if (row.leaseExpiresAt !== null && row.leaseExpiresAt <= nowMs) return false;
    return true;
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

  private recordAnomalyUnlocked(row: LedgerRow, message: string): void {
    if (!row.anomalies.includes(message)) row.anomalies.push(message);
  }
}

function cloneRow(row: LedgerRow): LedgerRow {
  return {
    ...row,
    incidents: [...row.incidents],
    anomalies: [...row.anomalies],
    storedResponse: row.storedResponse ? { ...row.storedResponse } : null,
    receipt: row.receipt ? { ...row.receipt } : null,
    paymentPayload: structuredClone(row.paymentPayload),
  };
}

export type { FulfillmentState, PaymentState, GatewayStatus };
