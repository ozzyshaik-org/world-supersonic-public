/**
 * FAKE upstream Go API — synthetic fixture only.
 * Does not follow redirects; status is returned as-is (redirect:'manual' semantics).
 */

import type { UpstreamClient, UpstreamRequest, UpstreamResponse } from "./client.js";

export type FakeUpstreamMode =
  | { kind: "success"; body?: unknown; status?: number }
  | { kind: "retryable"; message?: string }
  | { kind: "permanent"; status?: number; body?: unknown }
  | { kind: "redirect"; status?: number; location?: string }
  | { kind: "timeout" }
  | {
      kind: "sequence";
      modes: FakeUpstreamMode[];
    };

export class FakeUpstreamClient implements UpstreamClient {
  readonly name = "FAKE_UpstreamClient";
  calls: UpstreamRequest[] = [];
  mode: FakeUpstreamMode;
  modeOnce: FakeUpstreamMode | null = null;
  private sequenceIndex = 0;
  /** When set, the next `holdCount` fetches await releaseGate(). */
  private gate: Promise<void> | null = null;
  private gateResolve: (() => void) | null = null;
  private holdCount = 0;
  /** Resolves when the Nth fetch has entered the gate (1-based). */
  private enteredResolvers: Array<() => void> = [];

  constructor(mode: FakeUpstreamMode = { kind: "success" }) {
    this.mode = mode;
  }

  /**
   * Hold the next `count` fetch calls until releaseGate() is called.
   * Subsequent fetches after `count` proceed immediately (for takeover workers).
   */
  armGate(count = 1): void {
    this.holdCount = count;
    this.gate = new Promise((r) => {
      this.gateResolve = r;
    });
  }

  releaseGate(): void {
    this.gateResolve?.();
    this.gate = null;
    this.gateResolve = null;
    this.holdCount = 0;
  }

  /** Wait until at least `n` fetch calls have started (including gated ones). */
  whenCalls(n: number): Promise<void> {
    if (this.calls.length >= n) return Promise.resolve();
    return new Promise((resolve) => {
      this.enteredResolvers.push(() => {
        if (this.calls.length >= n) resolve();
      });
      // Also poll in case a call landed between check and push
      const iv = setInterval(() => {
        if (this.calls.length >= n) {
          clearInterval(iv);
          resolve();
        }
      }, 5);
    });
  }

  async fetch(req: UpstreamRequest): Promise<UpstreamResponse> {
    this.calls.push(req);
    for (const r of this.enteredResolvers) r();

    if (this.gate && this.holdCount > 0) {
      this.holdCount -= 1;
      await this.gate;
    }

    let mode = this.modeOnce ?? this.mode;
    this.modeOnce = null;

    if (mode.kind === "sequence") {
      const next = mode.modes[this.sequenceIndex] ?? mode.modes[mode.modes.length - 1]!;
      this.sequenceIndex += 1;
      mode = next;
    }

    return this.dispatch(mode, req);
  }

  private async dispatch(
    mode: FakeUpstreamMode,
    req: UpstreamRequest,
  ): Promise<UpstreamResponse> {
    switch (mode.kind) {
      case "success":
        return {
          status: mode.status ?? 200,
          body:
            mode.body ??
            {
              fixture: "FAKE_upstream_response",
              path: req.path,
              query: req.rawQuery,
            },
          contentType: "application/json",
        };
      case "retryable": {
        const err = new Error(mode.message ?? "FAKE upstream timeout/5xx");
        (err as Error & { retryable: boolean }).retryable = true;
        throw err;
      }
      case "permanent":
        return {
          status: mode.status ?? 400,
          body: mode.body ?? { error: "FAKE_permanent_upstream_failure" },
          contentType: "application/json",
        };
      case "redirect":
        return {
          status: mode.status ?? 302,
          body: { location: mode.location ?? "https://example.invalid/elsewhere" },
          contentType: "application/json",
        };
      case "timeout": {
        const err = new Error("FAKE upstream timeout");
        (err as Error & { retryable: boolean }).retryable = true;
        throw err;
      }
      case "sequence":
        throw new Error("FAKE upstream nested sequence not supported");
      default: {
        const _exhaustive: never = mode;
        return _exhaustive;
      }
    }
  }
}
