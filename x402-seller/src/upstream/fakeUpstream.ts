/**
 * FAKE upstream Go API — synthetic fixture only.
 */

import type { UpstreamClient, UpstreamRequest, UpstreamResponse } from "./client.js";

export type FakeUpstreamMode =
  | { kind: "success"; body?: unknown; status?: number }
  | { kind: "retryable"; message?: string }
  | { kind: "permanent"; status?: number; body?: unknown }
  | { kind: "timeout" };

export class FakeUpstreamClient implements UpstreamClient {
  readonly name = "FAKE_UpstreamClient";
  calls: UpstreamRequest[] = [];
  mode: FakeUpstreamMode;
  modeOnce: FakeUpstreamMode | null = null;

  constructor(mode: FakeUpstreamMode = { kind: "success" }) {
    this.mode = mode;
  }

  async fetch(req: UpstreamRequest): Promise<UpstreamResponse> {
    this.calls.push(req);
    const mode = this.modeOnce ?? this.mode;
    this.modeOnce = null;

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
      case "timeout": {
        const err = new Error("FAKE upstream timeout");
        (err as Error & { retryable: boolean }).retryable = true;
        throw err;
      }
      default: {
        const _exhaustive: never = mode;
        return _exhaustive;
      }
    }
  }
}
