/**
 * Seller engine: validate → reserve → settle() → fulfill.
 * Never fulfill before Gateway acceptance. No seller batch worker.
 */

import { buildAuthKey, assertNonce32Bytes, AuthKeyError } from "./authKey.js";
import { computeFingerprintV1 } from "./fingerprint.js";
import type { GatewayClient } from "./gateway/client.js";
import { MemoryLedger, LedgerConflictError } from "./ledger/memoryLedger.js";
import { lookupPrice, DEFAULT_PRICE_TABLE } from "./priceTable.js";
import type { UpstreamClient } from "./upstream/client.js";
import type {
  AuthorizationBinding,
  LedgerRow,
  PaymentPayload,
  PaymentRequirements,
  PriceTableEntry,
  SellerRequest,
  SellerResult,
  SettleResponse,
  TransferRecord,
} from "./types.js";
import { isDirectBatchingEvidence, isLocallySettled } from "./types.js";

export interface SellerConfig {
  network: string;
  asset: string;
  payTo: string;
  priceTable?: readonly PriceTableEntry[];
  leaseMs?: number;
  attemptCap?: number;
  /** Permanent failure after this many fulfillment attempts. */
  fulfillmentRetryBudget?: number;
  clock?: () => Date;
}

export interface SellerDeps {
  gateway: GatewayClient;
  upstream: UpstreamClient;
  ledger?: MemoryLedger;
  config: SellerConfig;
}

function toInt(v: string | number): number {
  return typeof v === "number" ? v : Number.parseInt(String(v), 10);
}

function lower0x(addr: string): string {
  return addr.toLowerCase();
}

export class SellerEngine {
  readonly gateway: GatewayClient;
  readonly upstream: UpstreamClient;
  readonly ledger: MemoryLedger;
  readonly config: Required<
    Pick<SellerConfig, "leaseMs" | "attemptCap" | "fulfillmentRetryBudget">
  > &
    SellerConfig;
  readonly priceTable: readonly PriceTableEntry[];
  private readonly clock: () => Date;
  private workerSeq = 0;
  /** Tracks fulfillment attempts per auth_key for permanent-failure budget. */
  private fulfillmentAttempts = new Map<string, number>();

  constructor(deps: SellerDeps) {
    this.gateway = deps.gateway;
    this.upstream = deps.upstream;
    this.ledger = deps.ledger ?? new MemoryLedger();
    this.priceTable = deps.config.priceTable ?? DEFAULT_PRICE_TABLE;
    this.config = {
      ...deps.config,
      leaseMs: deps.config.leaseMs ?? 5_000,
      attemptCap: deps.config.attemptCap ?? 3,
      fulfillmentRetryBudget: deps.config.fulfillmentRetryBudget ?? 3,
    };
    this.clock = deps.config.clock ?? (() => new Date());
  }

  now(): Date {
    return this.clock();
  }

  nowIso(): string {
    return this.now().toISOString();
  }

  nowSec(): number {
    return Math.floor(this.now().getTime() / 1000);
  }

  buildRequirements(entry: PriceTableEntry): PaymentRequirements {
    return {
      scheme: "exact",
      network: this.config.network,
      asset: this.config.asset,
      amount: entry.price,
      payTo: this.config.payTo,
      maxTimeoutSeconds: 345600,
      extra: {
        name: "GatewayWalletBatched",
        version: "1",
        verifyingContract: "0x0077777d7EBA4688BDeF3E311b846F25870A19B9",
      },
    };
  }

  async handle(req: SellerRequest): Promise<SellerResult> {
    return this.ledger.withLock(() => this.handleLocked(req));
  }

  private async handleLocked(req: SellerRequest): Promise<SellerResult> {
    const entry = lookupPrice(this.priceTable, req.path);

    // Method / body / path / fingerprint validation before any ledger or Gateway call
    if (req.method !== "GET") {
      return { httpStatus: 405, code: "method_not_allowed" };
    }
    if (req.bodyByteLength !== 0) {
      return { httpStatus: 400, code: "bad_request", body: { reason: "body not empty" } };
    }
    if (!entry) {
      return { httpStatus: 404, code: "not_found" };
    }
    if (entry.routeClass === "agent_pass") {
      // Agent Pass composition is out of first-checkpoint scope; refuse before settle.
      return {
        httpStatus: 403,
        code: "bad_request",
        body: { reason: "agent_pass routes are not enabled in this checkpoint" },
      };
    }

    const fp = computeFingerprintV1({
      method: req.method,
      path: req.path,
      rawQuery: req.rawQuery,
      bodyByteLength: req.bodyByteLength,
      allowedQueryParams: entry.queryParams,
      pathAllowed: true,
    });
    if (!fp.ok) {
      return {
        httpStatus: fp.httpStatus,
        code:
          fp.httpStatus === 405
            ? "method_not_allowed"
            : fp.httpStatus === 404
              ? "not_found"
              : "bad_request",
        body: { reason: fp.reason },
      };
    }

    if (!req.payment) {
      return {
        httpStatus: 402,
        code: "payment_required",
        paymentRequirements: this.buildRequirements(entry),
      };
    }

    let binding: AuthorizationBinding;
    try {
      binding = this.validatePaymentStructure(req.payment, entry, fp.fingerprint);
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      return { httpStatus: 400, code: "bad_request", body: { reason: msg } };
    }

    // Existing nonce / auth_key?
    const existingByNonce = this.ledger.getByNonce(binding.nonce);
    if (existingByNonce) {
      return this.handleExisting(existingByNonce, binding, req);
    }

    // Reserve
    let row: LedgerRow;
    try {
      row = this.ledger.insertReservation(binding, req.payment, this.nowIso());
    } catch (e) {
      if (e instanceof LedgerConflictError) {
        const again = this.ledger.getByNonce(binding.nonce);
        if (again) return this.handleExisting(again, binding, req);
      }
      throw e;
    }

    return this.submitAndMaybeFulfill(row, req);
  }

  private validatePaymentStructure(
    payment: PaymentPayload,
    entry: PriceTableEntry,
    fingerprint: string,
  ): AuthorizationBinding {
    const auth = payment.payload?.authorization;
    if (!auth) throw new Error("missing authorization");
    const nonce = assertNonce32Bytes(auth.nonce);
    const payer = lower0x(auth.from);
    const payTo = lower0x(auth.to);
    const value = String(auth.value);
    const validAfter = toInt(auth.validAfter);
    const validBefore = toInt(auth.validBefore);
    if (!Number.isFinite(validAfter) || !Number.isFinite(validBefore)) {
      throw new Error("invalid validity window");
    }
    if (payTo !== lower0x(this.config.payTo)) {
      throw new Error("payTo mismatch");
    }
    if (value !== entry.price) {
      throw new Error("amount mismatch");
    }
    const network = payment.network ?? this.config.network;
    if (network !== this.config.network) {
      throw new Error("network mismatch");
    }
    // asset is on requirements; payload may not carry it — enforced against config/price table
    const authKey = buildAuthKey(network, payer, nonce);
    return {
      authKey,
      nonce,
      network,
      asset: this.config.asset,
      payTo: lower0x(this.config.payTo),
      price: entry.price,
      payer,
      validAfter,
      validBefore,
      fingerprint,
    };
  }

  private bindingsEqual(a: AuthorizationBinding, b: AuthorizationBinding): boolean {
    return (
      a.authKey === b.authKey &&
      a.nonce === b.nonce &&
      a.network === b.network &&
      a.asset === b.asset &&
      a.payTo === b.payTo &&
      a.price === b.price &&
      a.payer === b.payer &&
      a.validAfter === b.validAfter &&
      a.validBefore === b.validBefore &&
      a.fingerprint === b.fingerprint
    );
  }

  private bindingMismatchField(
    stored: AuthorizationBinding,
    incoming: AuthorizationBinding,
  ): string | null {
    const fields: (keyof AuthorizationBinding)[] = [
      "payer",
      "network",
      "asset",
      "payTo",
      "price",
      "validAfter",
      "validBefore",
      "fingerprint",
    ];
    for (const f of fields) {
      if (stored[f] !== incoming[f]) return f;
    }
    return null;
  }

  private async handleExisting(
    stored: LedgerRow,
    incoming: AuthorizationBinding,
    req: SellerRequest,
  ): Promise<SellerResult> {
    const mismatch = this.bindingMismatchField(stored, incoming);
    if (mismatch) {
      return {
        httpStatus: 409,
        code: "authorization_binding_mismatch",
        body: { field: mismatch },
        authKey: stored.authKey,
      };
    }

    // Auto-void check for never-submitted pending rows
    if (stored.paymentState === "submission_pending" && stored.attemptCounter === 0) {
      const voided = this.ledger.tryAutomaticVoid(stored.authKey, this.nowSec(), this.nowIso());
      if (voided) {
        return {
          httpStatus: 409,
          code: "authorization_void",
          authKey: stored.authKey,
        };
      }
    }

    const row = this.ledger.getByAuthKey(stored.authKey)!;

    switch (row.paymentState) {
      case "void":
        return { httpStatus: 409, code: "authorization_void", authKey: row.authKey };
      case "rejected":
        return {
          httpStatus: 402,
          code: "payment_rejected",
          authKey: row.authKey,
          gatewayErrorReason: row.gatewayError ?? undefined,
        };
      case "submission_uncertain":
        return this.reconcileUncertain(row, req);
      case "submission_pending":
        if (row.attemptCounter === 0) {
          return this.submitAndMaybeFulfill(row, req);
        }
        // Unresolved attempt — treat as uncertain path (rule 4/5)
        return this.reconcileUncertain(
          this.ledger.updatePaymentState(
            row.authKey,
            "submission_uncertain",
            {},
            this.nowIso(),
          ),
          req,
        );
      case "accepted":
        return this.fulfillAccepted(row, req);
      default: {
        const _e: never = row.paymentState;
        return _e;
      }
    }
  }

  private async submitAndMaybeFulfill(
    row: LedgerRow,
    req: SellerRequest,
  ): Promise<SellerResult> {
    // Only one submitter
    const claimed = this.ledger.claimSubmitter(row.authKey, row.attemptCounter, this.nowIso());
    if (!claimed) {
      const latest = this.ledger.getByAuthKey(row.authKey)!;
      if (latest.paymentState === "submission_uncertain" || latest.attemptCounter > 0) {
        return {
          httpStatus: 503,
          code: "payment_submission_unresolved",
          authKey: latest.authKey,
        };
      }
      return {
        httpStatus: 503,
        code: "payment_submission_unresolved",
        authKey: row.authKey,
      };
    }

    const requirements = this.buildRequirements(lookupPrice(this.priceTable, req.path)!);
    let settleResult: SettleResponse;
    try {
      settleResult = await this.gateway.settle(claimed.paymentPayload, requirements);
    } catch (e) {
      const httpStatus = (e as { httpStatus?: number }).httpStatus;
      const errorReason = (e as { errorReason?: string }).errorReason;
      if (httpStatus === 400) {
        this.ledger.updatePaymentState(
          claimed.authKey,
          "rejected",
          { gatewayError: errorReason ?? "invalid_payload" },
          this.nowIso(),
        );
        return {
          httpStatus: 402,
          code: "payment_rejected",
          authKey: claimed.authKey,
          gatewayErrorReason: errorReason ?? "invalid_payload",
        };
      }
      // timeout / drop / empty body → uncertain
      this.ledger.updatePaymentState(claimed.authKey, "submission_uncertain", {}, this.nowIso());
      return {
        httpStatus: 503,
        code: "payment_submission_unresolved",
        authKey: claimed.authKey,
      };
    }

    if (settleResult.success === true && settleResult.transaction) {
      const accepted = this.ledger.updatePaymentState(
        claimed.authKey,
        "accepted",
        {
          gatewayTransferId: settleResult.transaction,
          gatewayStatus: null,
        },
        this.nowIso(),
      );
      return this.fulfillAccepted(accepted, req);
    }

    // success:false
    const reason = settleResult.errorReason ?? "unknown";
    if (reason === "nonce_already_used" || reason === "unexpected_error") {
      this.ledger.updatePaymentState(
        claimed.authKey,
        "submission_uncertain",
        { gatewayError: reason },
        this.nowIso(),
      );
      return {
        httpStatus: 503,
        code: "payment_submission_unresolved",
        authKey: claimed.authKey,
        gatewayErrorReason: reason,
      };
    }

    this.ledger.updatePaymentState(
      claimed.authKey,
      "rejected",
      { gatewayError: reason },
      this.nowIso(),
    );
    return {
      httpStatus: 402,
      code: "payment_rejected",
      authKey: claimed.authKey,
      gatewayErrorReason: reason,
    };
  }

  private async reconcileUncertain(
    row: LedgerRow,
    req: SellerRequest,
  ): Promise<SellerResult> {
    // Look up by transfer id if known, else by nonce with explicit network
    let found: TransferRecord | null = null;
    if (row.gatewayTransferId) {
      found = await this.gateway.getTransferById(row.gatewayTransferId);
    }
    if (!found) {
      const hits = await this.gateway.searchTransfers({
        nonce: row.nonce,
        network: row.network,
      });
      found = hits[0] ?? null;
    }

    if (found) {
      const match =
        found.nonce.toLowerCase() === row.nonce.toLowerCase() &&
        found.fromAddress.toLowerCase() === row.payer.toLowerCase() &&
        found.toAddress.toLowerCase() === row.payTo.toLowerCase() &&
        found.amount === row.price;

      if (!match) {
        this.ledger.updatePaymentState(
          row.authKey,
          "rejected",
          { gatewayError: "nonce_consumed_elsewhere" },
          this.nowIso(),
        );
        return {
          httpStatus: 402,
          code: "payment_rejected",
          authKey: row.authKey,
          gatewayErrorReason: "nonce_consumed_elsewhere",
        };
      }

      let accepted = this.ledger.updatePaymentState(
        row.authKey,
        "accepted",
        { gatewayTransferId: found.id },
        this.nowIso(),
      );
      accepted = this.ledger.applyGatewayStatus(row.authKey, found.status, this.nowIso());
      return this.fulfillAccepted(accepted, req);
    }

    // Empty lookup — NOT proof of non-acceptance
    const windowOpen = this.nowSec() < row.validBefore;
    if (windowOpen && row.attemptCounter < this.config.attemptCap) {
      // Bounded nonce-deduplicated retry: back to submission_pending
      this.ledger.updatePaymentState(row.authKey, "submission_pending", {}, this.nowIso());
      const pending = this.ledger.getByAuthKey(row.authKey)!;
      return this.submitAndMaybeFulfill(pending, req);
    }

    // Unresolved: stay uncertain, flag operator, never void
    this.ledger.addIncident(row.authKey, "submission_unresolved", this.nowIso());
    // Ensure still uncertain (never void)
    const latest = this.ledger.getByAuthKey(row.authKey)!;
    if (latest.paymentState === "submission_uncertain" || latest.attemptCounter > 0) {
      if (latest.paymentState !== "submission_uncertain") {
        this.ledger.updatePaymentState(
          latest.authKey,
          "submission_uncertain",
          {},
          this.nowIso(),
        );
      }
    }
    return {
      httpStatus: 503,
      code: "payment_submission_unresolved",
      authKey: row.authKey,
    };
  }

  private async fulfillAccepted(
    row: LedgerRow,
    req: SellerRequest,
  ): Promise<SellerResult> {
    // Observe gateway status when transfer id known (before fulfill / on retry)
    if (row.gatewayTransferId) {
      const t = await this.gateway.getTransferById(row.gatewayTransferId);
      if (t) {
        row = this.ledger.applyGatewayStatus(row.authKey, t.status, this.nowIso());
      }
    }

    if (row.gatewayStatus === "failed") {
      if (row.fulfillmentState === "fulfilled") {
        this.ledger.addIncident(row.authKey, "fulfilled_but_unpaid", this.nowIso());
        const latest = this.ledger.getByAuthKey(row.authKey)!;
        return {
          httpStatus: latest.storedResponse!.status,
          code: "ok",
          body: {
            data: latest.storedResponse!.body,
            receipt: latest.receipt,
            incident: "fulfilled_but_unpaid",
          },
          authKey: row.authKey,
        };
      }
      return {
        httpStatus: 402,
        code: "payment_failed",
        authKey: row.authKey,
      };
    }

    if (row.fulfillmentState === "fulfilled") {
      const latest = this.ledger.getByAuthKey(row.authKey)!;
      return {
        httpStatus: latest.storedResponse!.status,
        code: "ok",
        body: { data: latest.storedResponse!.body, receipt: latest.receipt },
        authKey: row.authKey,
      };
    }

    if (row.fulfillmentState === "failed_permanent") {
      return {
        httpStatus: 502,
        code: "paid_unfulfilled",
        authKey: row.authKey,
      };
    }

    const owner = `worker-${++this.workerSeq}`;
    const leased = this.ledger.takeLease(
      row.authKey,
      owner,
      this.config.leaseMs,
      this.now().getTime(),
      this.nowIso(),
    );
    if (!leased) {
      const latest = this.ledger.getByAuthKey(row.authKey)!;
      if (latest.fulfillmentState === "fulfilled") {
        return {
          httpStatus: latest.storedResponse!.status,
          code: "ok",
          body: { data: latest.storedResponse!.body, receipt: latest.receipt },
          authKey: row.authKey,
        };
      }
      if (latest.fulfillmentState === "in_progress") {
        // Waiter path simplified for checkpoint: instruct retry
        return {
          httpStatus: 503,
          code: "upstream_error",
          body: { reason: "fulfillment_in_progress" },
          authKey: row.authKey,
        };
      }
      return {
        httpStatus: 503,
        code: "upstream_error",
        authKey: row.authKey,
      };
    }

    // Settle-before-fulfill invariant: gateway.settle must already have been called
    // (attemptCounter >= 1 and payment accepted). Upstream call happens only here.
    try {
      const upstream = await this.upstream.fetch({
        method: "GET",
        path: req.path,
        rawQuery: req.rawQuery,
      });

      if (upstream.status >= 400 && upstream.status < 500) {
        // Non-retryable upstream after acceptance
        this.ledger.markFailedPermanent(row.authKey, this.nowIso());
        return {
          httpStatus: 502,
          code: "paid_unfulfilled",
          authKey: row.authKey,
        };
      }
      if (upstream.status >= 500) {
        return this.onRetryableFulfillment(row.authKey, owner);
      }

      const committed = this.ledger.commitFulfillment(
        row.authKey,
        owner,
        {
          status: upstream.status,
          body: upstream.body,
          contentType: upstream.contentType,
        },
        {
          authKey: row.authKey,
          gatewayTransferId: row.gatewayTransferId,
          fingerprint: row.fingerprint,
          fulfilledAt: this.nowIso(),
        },
        this.nowIso(),
      );
      if (!committed) {
        // Stale lease — discard
        const latest = this.ledger.getByAuthKey(row.authKey)!;
        if (latest.storedResponse) {
          return {
            httpStatus: latest.storedResponse.status,
            code: "ok",
            body: { data: latest.storedResponse.body, receipt: latest.receipt },
            authKey: row.authKey,
          };
        }
        return {
          httpStatus: 503,
          code: "upstream_error",
          body: { reason: "stale_lease" },
          authKey: row.authKey,
        };
      }
      return {
        httpStatus: upstream.status,
        code: "ok",
        body: { data: upstream.body, receipt: committed.receipt },
        authKey: row.authKey,
      };
    } catch {
      return this.onRetryableFulfillment(row.authKey, owner);
    }
  }

  private onRetryableFulfillment(authKey: string, owner: string): SellerResult {
    this.ledger.markRetryableFailure(authKey, owner, this.nowIso());
    const n = (this.fulfillmentAttempts.get(authKey) ?? 0) + 1;
    this.fulfillmentAttempts.set(authKey, n);
    if (n >= this.config.fulfillmentRetryBudget) {
      this.ledger.markFailedPermanent(authKey, this.nowIso());
      return {
        httpStatus: 502,
        code: "paid_unfulfilled",
        authKey,
      };
    }
    return {
      httpStatus: 503,
      code: "upstream_error",
      body: { reason: "retryable_failure" },
      authKey,
    };
  }

  /**
   * Periodic observation helper (not a batch worker). Copies Gateway status forward.
   */
  async observeTransfer(authKey: string): Promise<LedgerRow | null> {
    return this.ledger.withLock(async () => {
      const row = this.ledger.getByAuthKey(authKey);
      if (!row || row.paymentState !== "accepted") return row ?? null;
      let found: TransferRecord | null = null;
      if (row.gatewayTransferId) {
        found = await this.gateway.getTransferById(row.gatewayTransferId);
      }
      if (!found) {
        const hits = await this.gateway.searchTransfers({
          nonce: row.nonce,
          network: row.network,
        });
        found = hits[0] ?? null;
      }
      if (!found) return row;

      let updated = this.ledger.applyGatewayStatus(authKey, found.status, this.nowIso());
      if (found.status === "failed" && updated.fulfillmentState === "fulfilled") {
        updated = this.ledger.addIncident(authKey, "fulfilled_but_unpaid", this.nowIso());
      }
      return updated;
    });
  }

  /** Exported for tests / docs — settlement helpers. */
  static isLocallySettled = isLocallySettled;
  static isDirectBatchingEvidence = isDirectBatchingEvidence;
}

export { AuthKeyError, isLocallySettled, isDirectBatchingEvidence };
