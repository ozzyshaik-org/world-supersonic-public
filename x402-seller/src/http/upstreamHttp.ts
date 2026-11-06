/**
 * Real HTTP upstream client for the Go API.
 * - redirect: 'manual' (never followed; 3xx returned as status)
 * - AbortController timeout
 * - Server-side X-API-Key from config only; never forwards client headers
 * - Does not log or echo the API key
 */

import type { UpstreamClient, UpstreamRequest, UpstreamResponse } from "../upstream/client.js";

export interface HttpUpstreamConfig {
  /** Absolute base URL, e.g. http://127.0.0.1:8081 — from env only. */
  baseUrl: string;
  /** Server-held Go API key. Never logged. */
  apiKey: string;
  /** Fetch timeout in milliseconds. */
  timeoutMs: number;
  /** Optional fetch impl (tests / injected). Defaults to global fetch. */
  fetchImpl?: typeof fetch;
}

export class HttpUpstreamClient implements UpstreamClient {
  readonly name = "HttpUpstreamClient";
  readonly calls: UpstreamRequest[] = [];
  private readonly baseUrl: string;
  private readonly apiKey: string;
  private readonly timeoutMs: number;
  private readonly fetchImpl: typeof fetch;

  constructor(config: HttpUpstreamConfig) {
    if (!config.baseUrl) {
      throw new Error("HttpUpstreamClient requires baseUrl");
    }
    this.baseUrl = config.baseUrl.replace(/\/+$/, "");
    this.apiKey = config.apiKey;
    this.timeoutMs = config.timeoutMs;
    this.fetchImpl = config.fetchImpl ?? fetch;
  }

  async fetch(req: UpstreamRequest): Promise<UpstreamResponse> {
    this.calls.push(req);
    const url =
      req.rawQuery.length > 0
        ? `${this.baseUrl}${req.path}?${req.rawQuery}`
        : `${this.baseUrl}${req.path}`;

    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), this.timeoutMs);
    try {
      const res = await this.fetchImpl(url, {
        method: "GET",
        redirect: "manual",
        signal: controller.signal,
        headers: {
          // Server-side only. Client-supplied Authorization / X-API-Key / api_key
          // are never accepted here — this client does not take request headers.
          "X-API-Key": this.apiKey,
          Accept: "application/json",
        },
      });

      const contentType = res.headers.get("content-type") ?? "application/octet-stream";
      const text = await res.text();
      let body: unknown = text;
      if (contentType.includes("application/json")) {
        try {
          body = text.length === 0 ? null : JSON.parse(text);
        } catch {
          body = text;
        }
      }
      return { status: res.status, body, contentType };
    } catch (e) {
      const err = e instanceof Error ? e : new Error(String(e));
      (err as Error & { retryable: boolean }).retryable = true;
      throw err;
    } finally {
      clearTimeout(timer);
    }
  }
}
