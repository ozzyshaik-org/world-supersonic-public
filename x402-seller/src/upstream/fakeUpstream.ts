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
  /** Barrier for lost-response / concurrent tests: resolves after Nth call starts. */
  gate: Promise<void> | null = null;
  private gateResolve: (() => void) | null = null;

  constructor(mode: FakeUpstreamMode = { kind: "success" }) {
    this.mode = mode;
  }

  /** Hold the next fetch until releaseGate() is called. */
  armGate(): void {
    this.gate = new Promise((r) => {
      this.gateResolve = r;
    });
  }

  releaseGate(): void {
    this.gateResolve?.();
    this.gate = null;
    this.gateResolve = null;
  }

  async fetch(req: UpstreamRequest): Promise<UpstreamResponse> {
    this.calls.push(req);
    if (this.gate) {
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
        // redirect:'manual' — surface 302 without following
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
