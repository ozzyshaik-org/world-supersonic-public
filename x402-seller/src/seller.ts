/**
 * Seller engine: validate → reserve → settle() → fulfill.
 * Never fulfill before Gateway acceptance. No seller batch worker.
 * Ledger mutex is never held across Gateway or upstream network calls.
 */

import { randomUUID } from "node:crypto";
import { AuthKeyError } from "./authKey.js";
import { ARC_TESTNET_GATEWAY_WALLET } from "./chainConfig.js";
import { computeFingerprintV1 } from "./fingerprint.js";
import type { GatewayClient } from "./gateway/client.js";
import { MemoryLedger, LedgerConflictError } from "./ledger/memoryLedger.js";
import {
  bindingMismatchField,
  buildPaymentRequirements,
  parseCandidateBinding,
  PaymentValidationError,
  validateNewPayment,
} from "./paymentValidation.js";
import { lookupPrice, DEFAULT_PRICE_TABLE } from "./priceTable.js";
import {
  classifySearchHits,
  classifyTransferById,
  type LookupOutcome,
} from "./transferMatch.js";
import type { UpstreamClient } from "./upstream/client.js";
import type {
  AuthorizationBinding,
  LedgerRow,
  PaymentPayload,
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
  /** GatewayWallet verifyingContract from validated/official chain config. */
  verifyingContract?: string;
  priceTable?: readonly PriceTableEntry[];
  leaseMs?: number;
  attemptCap?: number;
  /** Permanent failure after this many fulfillment attempts. */
  fulfillmentRetryBudget?: number;
  /** Total-time retry budget in ms (spec rule 12). */
  fulfillmentRetryBudgetMs?: number;
  /** Default Retry-After seconds for unresolved 503s. */
  unresolvedRetryAfterSeconds?: number;
  /** Max wait for an in-progress lease holder before takeover check. */
  leaseWaitMs?: number;
  clock?: () => Date;
}

export interface SellerTestHooks {
  /**
   * Invoked after a successful resolveRetryableUnderLease that left the row
   * retryable_failure (lease already released). Tests use this for barriers;
   * production leaves unset. Never used to continue fencing after release.
   */
  afterRetryableRelease?: (row: LedgerRow) => void | Promise<void>;
}

export interface SellerDeps {
  gateway: GatewayClient;
  upstream: UpstreamClient;
  ledger?: MemoryLedger;
  config: SellerConfig;
  /** Test-only barriers; must not be set in production paths. */
  testHooks?: SellerTestHooks;
}

const DEFAULT_RETRY_AFTER = 2;
const LEASE_POLL_MS = 25;

export class SellerEngine {
  readonly gateway: GatewayClient;
  readonly upstream: UpstreamClient;
  readonly ledger: MemoryLedger;
  readonly config: Required<
    Pick<
      SellerConfig,
      | "leaseMs"
      | "attemptCap"
      | "fulfillmentRetryBudget"
      | "fulfillmentRetryBudgetMs"
      | "unresolvedRetryAfterSeconds"
      | "leaseWaitMs"
      | "verifyingContract"
    >
  > &
    SellerConfig;
  readonly priceTable: readonly PriceTableEntry[];
  private readonly clock: () => Date;

  constructor(deps: SellerDeps) {
    this.gateway = deps.gateway;
    this.upstream = deps.upstream;
    this.ledger = deps.ledger ?? new MemoryLedger();
    this.priceTable = deps.config.priceTable ?? DEFAULT_PRICE_TABLE;
    this.config = {
      ...deps.config,
      verifyingContract: deps.config.verifyingContract ?? ARC_TESTNET_GATEWAY_WALLET,
      leaseMs: deps.config.leaseMs ?? 5_000,
      attemptCap: deps.config.attemptCap ?? 3,
      fulfillmentRetryBudget: deps.config.fulfillmentRetryBudget ?? 3,
      fulfillmentRetryBudgetMs: deps.config.fulfillmentRetryBudgetMs ?? 60_000,
      unresolvedRetryAfterSeconds:
        deps.config.unresolvedRetryAfterSeconds ?? DEFAULT_RETRY_AFTER,
      leaseWaitMs: deps.config.leaseWaitMs ?? 2_000,
    };
    this.clock = deps.config.clock ?? (() => new Date());
    this.testHooks = deps.testHooks;
  }

  private readonly testHooks: SellerTestHooks | undefined;

  now(): Date {
    return this.clock();
  }

  nowIso(): string {
    return this.now().toISOString();
  }

  nowSec(): number {
    return Math.floor(this.now().getTime() / 1000);
  }

  nowMs(): number {
    return this.now().getTime();
  }

  /**
   * Wall-clock ms for lease expiry / takeover. Lease fencing must advance with
   * real time so waiters sleeping outside the mutex can observe expiry even when
   * the injectable business clock is frozen in tests.
   */
  wallClockMs(): number {
    return Date.now();
  }

  private terms() {
    return {
      network: this.config.network,
      asset: this.config.asset,
      payTo: this.config.payTo,
      verifyingContract: this.config.verifyingContract,
    };
  }

  buildRequirements(entry: PriceTableEntry) {
    return buildPaymentRequirements(entry, this.terms());
  }

  private unresolved503(authKey: string, gatewayErrorReason?: string): SellerResult {
    return {
      httpStatus: 503,
      code: "payment_submission_unresolved",
      authKey,
      retryAfterSeconds: this.config.unresolvedRetryAfterSeconds,
      ...(gatewayErrorReason ? { gatewayErrorReason } : {}),
    };
  }

  /**
   * Public entry. Short ledger critical sections only; network waits are outside.
   */
  async handle(req: SellerRequest): Promise<SellerResult> {
    const entry = lookupPrice(this.priceTable, req.path);

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

    // Existing-nonce path: parse candidate binding first, apply binding rule
    // BEFORE ordinary price-table mismatch rejection.
    let candidate: AuthorizationBinding;
    try {
      candidate = parseCandidateBinding(req.payment, fp.fingerprint);
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      const status = e instanceof PaymentValidationError ? e.httpStatus : 400;
      return { httpStatus: status, code: "bad_request", body: { reason: msg } };
    }

    const existingByNonce = await this.ledger.withLock(() =>
      this.ledger.getByNonce(candidate.nonce),
    );

    if (existingByNonce) {
      const mismatch = bindingMismatchField(existingByNonce, candidate);
      if (mismatch) {
        return {
          httpStatus: 409,
          code: "authorization_binding_mismatch",
          body: { field: mismatch },
          authKey: existingByNonce.authKey,
        };
      }
      return this.handleExisting(existingByNonce, candidate, req);
    }

    // NEW nonce: full route-term validation before reservation
    let binding: AuthorizationBinding;
    try {
      binding = validateNewPayment(req.payment, entry, fp.fingerprint, {
        ...this.terms(),
        nowSec: this.nowSec(),
      });
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      const status = e instanceof PaymentValidationError ? e.httpStatus : 400;
      return { httpStatus: status, code: "bad_request", body: { reason: msg } };
    }

    let row: LedgerRow;
    try {
      row = await this.ledger.withLock(() =>
        this.ledger.insertReservation(binding, req.payment!, this.nowIso()),
      );
    } catch (e) {
      if (e instanceof LedgerConflictError) {
        const again = await this.ledger.withLock(() => this.ledger.getByNonce(binding.nonce));
        if (again) return this.handleExisting(again, binding, req);
      }
      throw e;
    }

    return this.submitAndMaybeFulfill(row, req);
  }

  private async handleExisting(
    stored: LedgerRow,
    incoming: AuthorizationBinding,
    req: SellerRequest,
  ): Promise<SellerResult> {
    // Auto-void check for never-submitted pending rows
    if (stored.paymentState === "submission_pending" && stored.attemptCounter === 0) {
      const voided = await this.ledger.withLock(() =>
        this.ledger.tryAutomaticVoid(stored.authKey, this.nowSec(), this.nowIso()),
      );
      if (voided) {
        return {
          httpStatus: 409,
          code: "authorization_void",
          authKey: stored.authKey,
        };
      }
    }

    const row = (await this.ledger.withLock(() =>
      this.ledger.getByAuthKey(stored.authKey),
    ))!;

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
        {
          const uncertain = await this.ledger.withLock(() =>
            this.ledger.updatePaymentState(
              row.authKey,
              "submission_uncertain",
              {},
              this.nowIso(),
            ),
          );
          return this.reconcileUncertain(uncertain, req);
        }
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
    const claimed = await this.ledger.withLock(() =>
      this.ledger.claimSubmitter(row.authKey, row.attemptCounter, this.nowIso()),
    );
    if (!claimed) {
      const latest = (await this.ledger.withLock(() =>
        this.ledger.getByAuthKey(row.authKey),
      ))!;
      return this.unresolved503(latest.authKey);
    }

    const requirements = this.buildRequirements(lookupPrice(this.priceTable, req.path)!);
    let settleResult: SettleResponse | null = null;
    let settleError: unknown = null;
    try {
      // Network call OUTSIDE ledger lock
      settleResult = await this.gateway.settle(claimed.paymentPayload, requirements);
    } catch (e) {
      settleError = e;
    }

    if (settleError) {
      const httpStatus = (settleError as { httpStatus?: number }).httpStatus;
      const errorReason = (settleError as { errorReason?: string }).errorReason;
      if (httpStatus === 400) {
        await this.ledger.withLock(() =>
          this.ledger.updatePaymentState(
            claimed.authKey,
            "rejected",
            { gatewayError: errorReason ?? "invalid_payload" },
            this.nowIso(),
          ),
        );
        return {
          httpStatus: 402,
          code: "payment_rejected",
          authKey: claimed.authKey,
          gatewayErrorReason: errorReason ?? "invalid_payload",
        };
      }
      // timeout / drop / empty body / HTTP 500 / unexpected transport → uncertain
      await this.ledger.withLock(() =>
        this.ledger.updatePaymentState(
          claimed.authKey,
          "submission_uncertain",
          {},
          this.nowIso(),
        ),
      );
      return this.unresolved503(claimed.authKey);
    }

    const result = settleResult!;

    // Accepted: success===true AND non-empty transaction
    if (result.success === true) {
      if (typeof result.transaction === "string" && result.transaction.length > 0) {
        const accepted = await this.ledger.withLock(() =>
          this.ledger.updatePaymentState(
            claimed.authKey,
            "accepted",
            {
              gatewayTransferId: result.transaction,
              gatewayStatus: null,
            },
            this.nowIso(),
          ),
        );
        return this.fulfillAccepted(accepted, req);
      }
      // success:true without transaction → uncertain, NOT definite rejection
      await this.ledger.withLock(() =>
        this.ledger.updatePaymentState(
          claimed.authKey,
          "submission_uncertain",
          { gatewayError: "malformed_settle_success_missing_transaction" },
          this.nowIso(),
        ),
      );
      return this.unresolved503(claimed.authKey, "malformed_settle_success_missing_transaction");
    }

    // success:false
    if (result.success === false) {
      const reason = result.errorReason;
      if (reason === "nonce_already_used" || reason === "unexpected_error") {
        await this.ledger.withLock(() =>
          this.ledger.updatePaymentState(
            claimed.authKey,
            "submission_uncertain",
            { gatewayError: reason },
            this.nowIso(),
          ),
        );
        return this.unresolved503(claimed.authKey, reason);
      }
      if (typeof reason === "string" && reason.length > 0) {
        // Definite rejection (HTTP 200 success:false with errorReason)
        await this.ledger.withLock(() =>
          this.ledger.updatePaymentState(
            claimed.authKey,
            "rejected",
            { gatewayError: reason },
            this.nowIso(),
          ),
        );
        return {
          httpStatus: 402,
          code: "payment_rejected",
          authKey: claimed.authKey,
          gatewayErrorReason: reason,
        };
      }
      // success:false without errorReason → uncertain
      await this.ledger.withLock(() =>
        this.ledger.updatePaymentState(
          claimed.authKey,
          "submission_uncertain",
          { gatewayError: "malformed_settle_false_missing_errorReason" },
          this.nowIso(),
        ),
      );
      return this.unresolved503(
        claimed.authKey,
        "malformed_settle_false_missing_errorReason",
      );
    }

    // Malformed / unexpected settle response shape
    await this.ledger.withLock(() =>
      this.ledger.updatePaymentState(
        claimed.authKey,
        "submission_uncertain",
        { gatewayError: "malformed_settle_response" },
        this.nowIso(),
      ),
    );
    return this.unresolved503(claimed.authKey, "malformed_settle_response");
  }

  /**
   * Look up a transfer for a row. Failures and ambiguity stay unresolved.
   * Uses centralized matching — never hits[0] as implicit disambiguation.
   */
  private async lookupTransfer(row: LedgerRow): Promise<LookupOutcome> {
    try {
      if (row.gatewayTransferId) {
        const byId = await this.gateway.getTransferById(row.gatewayTransferId);
        const classified = classifyTransferById(byId, row);
        if (classified.kind === "match" || classified.kind === "mismatch") {
          return classified;
        }
        // empty by id → fall through to search
      }
      const hits = await this.gateway.searchTransfers({
        nonce: row.nonce,
        network: row.network,
      });
      return classifySearchHits(hits, row);
    } catch (error) {
      return { kind: "failed", error };
    }
  }

  private async reconcileUncertain(
    row: LedgerRow,
    req: SellerRequest,
  ): Promise<SellerResult> {
    const outcome = await this.lookupTransfer(row);

    if (outcome.kind === "match") {
      const accepted = await this.ledger.withLock(() => {
        let r = this.ledger.updatePaymentState(
          row.authKey,
          "accepted",
          { gatewayTransferId: outcome.transfer.id },
          this.nowIso(),
        );
        r = this.ledger.applyGatewayStatus(row.authKey, outcome.transfer.status, this.nowIso());
        return r;
      });
      return this.fulfillAccepted(accepted, req);
    }

    if (outcome.kind === "mismatch") {
      await this.ledger.withLock(() =>
        this.ledger.updatePaymentState(
          row.authKey,
          "rejected",
          { gatewayError: "nonce_consumed_elsewhere" },
          this.nowIso(),
        ),
      );
      return {
        httpStatus: 402,
        code: "payment_rejected",
        authKey: row.authKey,
        gatewayErrorReason: "nonce_consumed_elsewhere",
      };
    }

    if (outcome.kind === "failed" || outcome.kind === "ambiguous") {
      // Fail closed: keep submission_uncertain, flag unresolved, no resubmit/fulfill
      await this.ledger.withLock(() => {
        this.ledger.addIncident(row.authKey, "submission_unresolved", this.nowIso());
        const latest = this.ledger.getByAuthKey(row.authKey)!;
        if (latest.paymentState !== "submission_uncertain") {
          this.ledger.updatePaymentState(
            latest.authKey,
            "submission_uncertain",
            {},
            this.nowIso(),
          );
        }
      });
      return this.unresolved503(row.authKey);
    }

    // Empty lookup — NOT proof of non-acceptance. Bounded retry only after
    // SUCCESSFUL empty lookup, while validity open and attempt cap unspent.
    const windowOpen = this.nowSec() < row.validBefore;
    if (windowOpen && row.attemptCounter < this.config.attemptCap) {
      const pending = await this.ledger.withLock(() =>
        this.ledger.updatePaymentState(row.authKey, "submission_pending", {}, this.nowIso()),
      );
      return this.submitAndMaybeFulfill(pending, req);
    }

    await this.ledger.withLock(() => {
      this.ledger.addIncident(row.authKey, "submission_unresolved", this.nowIso());
      const latest = this.ledger.getByAuthKey(row.authKey)!;
      if (latest.paymentState !== "submission_uncertain") {
        this.ledger.updatePaymentState(
          latest.authKey,
          "submission_uncertain",
          {},
          this.nowIso(),
        );
      }
    });
    return this.unresolved503(row.authKey);
  }

  private okFromStored(row: LedgerRow): SellerResult {
    return {
      httpStatus: row.storedResponse!.status,
      code: "ok",
      body: { data: row.storedResponse!.body, receipt: row.receipt },
      authKey: row.authKey,
    };
  }

  private async fulfillAccepted(
    row: LedgerRow,
    req: SellerRequest,
  ): Promise<SellerResult> {
    // Check fulfillmentState === "fulfilled" BEFORE any Gateway status lookup.
    // Same-binding fulfilled retry returns stored response even when Gateway is unavailable.
    {
      const latest = (await this.ledger.withLock(() =>
        this.ledger.getByAuthKey(row.authKey),
      ))!;
      if (latest.fulfillmentState === "fulfilled") {
        // Optionally refresh gateway status for operator metadata, but never alter response.
        // If Gateway lookup fails, still return stored response.
        if (latest.gatewayTransferId) {
          try {
            const t = await this.gateway.getTransferById(latest.gatewayTransferId);
            if (t) {
              const matched = classifyTransferById(t, latest);
              if (matched.kind === "match") {
                await this.ledger.withLock(() => {
                  this.ledger.applyGatewayStatus(
                    latest.authKey,
                    matched.transfer.status,
                    this.nowIso(),
                  );
                  if (
                    matched.transfer.status === "failed" &&
                    this.ledger.getByAuthKey(latest.authKey)!.fulfillmentState === "fulfilled"
                  ) {
                    this.ledger.addIncident(
                      latest.authKey,
                      "fulfilled_but_unpaid",
                      this.nowIso(),
                    );
                  }
                });
              }
              // mismatch / unknown: do not mutate; anomaly recorded inside apply if needed
            }
          } catch {
            // Gateway unavailable — still return stored response unchanged
          }
        }
        const fresh = (await this.ledger.withLock(() =>
          this.ledger.getByAuthKey(row.authKey),
        ))!;
        // fulfilled_but_unpaid lives in ledger/operator metadata, NOT in client response body
        return this.okFromStored(fresh);
      }

      if (latest.fulfillmentState === "failed_permanent") {
        return {
          httpStatus: 502,
          code: "paid_unfulfilled",
          authKey: row.authKey,
        };
      }

      // Observe gateway before starting fulfillment (not-yet-fulfilled paths)
      if (latest.gatewayTransferId) {
        try {
          const t = await this.gateway.getTransferById(latest.gatewayTransferId);
          if (t) {
            const matched = classifyTransferById(t, latest);
            if (matched.kind === "match") {
              row = await this.ledger.withLock(() =>
                this.ledger.applyGatewayStatus(
                  latest.authKey,
                  matched.transfer.status,
                  this.nowIso(),
                ),
              );
            }
          }
        } catch {
          // lookup failure: proceed without updating status
        }
      }

      if (row.gatewayStatus === "failed") {
        return {
          httpStatus: 402,
          code: "payment_failed",
          authKey: row.authKey,
        };
      }
    }

    // Wait for in-progress lease holder or take over after expiry.
    // Never return upstream_error merely because another caller holds the lease:
    // waiters poll outside the mutex until a stored result appears or the lease expires.
    while (true) {
      const snapshot = (await this.ledger.withLock(() =>
        this.ledger.getByAuthKey(row.authKey),
      ))!;

      if (snapshot.fulfillmentState === "fulfilled") {
        return this.okFromStored(snapshot);
      }
      if (snapshot.fulfillmentState === "failed_permanent") {
        return {
          httpStatus: 502,
          code: "paid_unfulfilled",
          authKey: row.authKey,
        };
      }
      if (snapshot.gatewayStatus === "failed") {
        return {
          httpStatus: 402,
          code: "payment_failed",
          authKey: row.authKey,
        };
      }

      const owner = randomUUID();
      const leased = await this.ledger.withLock(() =>
        this.ledger.takeLease(
          row.authKey,
          owner,
          this.config.leaseMs,
          this.wallClockMs(),
          this.nowIso(),
        ),
      );

      if (leased) {
        return this.runFulfillmentUnderLease(leased.row, leased.generation, owner, req);
      }

      // Another holder has a still-valid lease — sleep outside the mutex, then re-check.
      const again = (await this.ledger.withLock(() =>
        this.ledger.getByAuthKey(row.authKey),
      ))!;
      if (again.fulfillmentState === "fulfilled") {
        return this.okFromStored(again);
      }
      const expiresAt = again.leaseExpiresAt;
      const nowWall = this.wallClockMs();
      if (
        again.fulfillmentState === "in_progress" &&
        expiresAt !== null &&
        expiresAt > nowWall
      ) {
        const sleepMs = Math.min(LEASE_POLL_MS, Math.max(1, expiresAt - nowWall));
        await sleep(sleepMs);
        continue;
      }
      // Lease expired (or no expiry recorded) — loop to take over.
      await sleep(LEASE_POLL_MS);
    }
  }

  /**
   * After a stale/expired worker loses CAS, wait for the peer lease holder to
   * store a result (or for a permanent failure). Used so concurrent same-binding
   * callers receive the identical successful stored response.
   */
  private async waitForPeerResult(authKey: string): Promise<SellerResult> {
    const deadline =
      this.wallClockMs() + this.config.leaseMs + this.config.leaseWaitMs + 5_000;
    while (this.wallClockMs() < deadline) {
      const latest = (await this.ledger.withLock(() =>
        this.ledger.getByAuthKey(authKey),
      ))!;
      if (latest.fulfillmentState === "fulfilled" && latest.storedResponse) {
        return this.okFromStored(latest);
      }
      if (latest.fulfillmentState === "failed_permanent") {
        return {
          httpStatus: 502,
          code: "paid_unfulfilled",
          authKey,
        };
      }
      if (latest.gatewayStatus === "failed" && latest.fulfillmentState !== "fulfilled") {
        return {
          httpStatus: 402,
          code: "payment_failed",
          authKey,
        };
      }
      await sleep(LEASE_POLL_MS);
    }
    const final = (await this.ledger.withLock(() => this.ledger.getByAuthKey(authKey)))!;
    if (final.fulfillmentState === "fulfilled" && final.storedResponse) {
      return this.okFromStored(final);
    }
    return {
      httpStatus: 503,
      code: "upstream_error",
      body: { reason: "peer_fulfillment_timeout" },
      authKey,
      retryAfterSeconds: this.config.unresolvedRetryAfterSeconds,
    };
  }

  private async runFulfillmentUnderLease(

    row: LedgerRow,
    generation: number,
    owner: string,
    req: SellerRequest,
  ): Promise<SellerResult> {
    try {
      // Network call OUTSIDE ledger lock
      const upstream = await this.upstream.fetch({
        method: "GET",
        path: req.path,
        rawQuery: req.rawQuery,
      });

      // Only 200-299 may commitFulfillment. Do not follow redirects (fake/abstraction).
      if (upstream.status >= 200 && upstream.status <= 299) {
        const committed = await this.ledger.withLock(() =>
          this.ledger.commitFulfillment(
            row.authKey,
            owner,
            generation,
            this.wallClockMs(),
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
          ),
        );
        if (!committed) {
          // Stale / expired lease — discard upstream result; wait for peer stored response
          return this.waitForPeerResult(row.authKey);
        }
        return {
          httpStatus: upstream.status,
          code: "ok",
          body: { data: upstream.body, receipt: committed.receipt },
          authKey: row.authKey,
        };
      }

      // Non-2xx: 5xx / retryable vs permanent 4xx (and other non-2xx like 302)
      if (upstream.status >= 500) {
        return this.onRetryableFulfillment(row.authKey, owner, generation);
      }

      // Non-retryable (4xx, 302, etc.) after acceptance → failed_permanent + paid_unfulfilled
      const permanent = await this.ledger.withLock(() =>
        this.ledger.markFailedPermanent(row.authKey, this.nowIso(), {
          owner,
          generation,
          nowMs: this.wallClockMs(),
        }),
      );
      if (!permanent) {
        // Lost fence — do not apply permanent failure; await peer outcome
        return this.waitForPeerResult(row.authKey);
      }
      return {
        httpStatus: 502,
        code: "paid_unfulfilled",
        authKey: row.authKey,
      };
    } catch {
      return this.onRetryableFulfillment(row.authKey, owner, generation);
    }
  }

  private async onRetryableFulfillment(
    authKey: string,
    owner: string,
    generation: number,
  ): Promise<SellerResult> {
    // Atomic under the ORIGINAL lease: retryable_failure XOR failed_permanent.
    // Never markRetryableFailure (release) then unfenced markFailedPermanent.
    const resolved = await this.ledger.withLock(() =>
      this.ledger.resolveRetryableUnderLease(
        authKey,
        owner,
        generation,
        this.wallClockMs(),
        this.nowIso(),
        {
          maxAttempts: this.config.fulfillmentRetryBudget,
          maxMs: this.config.fulfillmentRetryBudgetMs,
          nowBusinessMs: this.nowMs(),
        },
      ),
    );
    // Lost CAS (peer takeover / expired lease): do not retry the transition
    // under a fence we no longer hold — defer to the peer's outcome.
    if (!resolved) {
      return this.waitForPeerResult(authKey);
    }
    if (resolved.exhausted) {
      return {
        httpStatus: 502,
        code: "paid_unfulfilled",
        authKey,
      };
    }
    if (this.testHooks?.afterRetryableRelease) {
      await this.testHooks.afterRetryableRelease(resolved.row);
    }
    return {
      httpStatus: 503,
      code: "upstream_error",
      body: { reason: "retryable_failure" },
      authKey,
      retryAfterSeconds: this.config.unresolvedRetryAfterSeconds,
    };
  }

  /**
   * Periodic observation helper (not a batch worker). Copies Gateway status forward
   * only via centralized matching. Unknown/backward → anomaly, no mutation.
   */
  async observeTransfer(authKey: string): Promise<LedgerRow | null> {
    const row = await this.ledger.withLock(() => this.ledger.getByAuthKey(authKey));
    if (!row || row.paymentState !== "accepted") return row ?? null;

    const outcome = await this.lookupTransfer(row);
    if (outcome.kind !== "match") {
      // failed / ambiguous / empty / mismatch: never apply a mismatched transfer's status
      return row;
    }

    return this.ledger.withLock(() => {
      let updated = this.ledger.applyGatewayStatus(
        authKey,
        outcome.transfer.status,
        this.nowIso(),
      );
      if (
        outcome.transfer.status === "failed" &&
        updated.fulfillmentState === "fulfilled"
      ) {
        updated = this.ledger.addIncident(authKey, "fulfilled_but_unpaid", this.nowIso());
      }
      return updated;
    });
  }

  static isLocallySettled = isLocallySettled;
  static isDirectBatchingEvidence = isDirectBatchingEvidence;
}

function sleep(ms: number): Promise<void> {
  return new Promise((r) => setTimeout(r, ms));
}

export { AuthKeyError, isLocallySettled, isDirectBatchingEvidence };
