/**
 * Thinnest real HTTP adapter around SellerEngine.
 * Experimental / local. Not live Arc evidence.
 *
 * - Only the allowlisted paid path is exposed (default /v1/mev/analytics).
 * - No generic proxy; encoded/traversal/other paths rejected without upstream.
 * - PAYMENT-SIGNATURE parsed here; malformed → 400 before engine/Gateway/upstream.
 * - 402 emits PAYMENT-REQUIRED (base64) + JSON body { x402Version, accepts }.
 * - 503 payment_submission_unresolved / retryable upstream emit Retry-After.
 * - Client Authorization / X-API-Key / api_key never forwarded to upstream.
 */

import http from "node:http";
import type { SellerEngine } from "../seller.js";
import type { PaymentPayload, SellerResult } from "../types.js";
import {
  HEADER_PAYMENT_REQUIRED,
  HEADER_PAYMENT_RESPONSE,
  HEADER_PAYMENT_SIGNATURE,
  PaymentHeaderError,
  buildPaymentRequiredEnvelope,
  decodePaymentSignatureHeader,
  encodePaymentRequiredHeader,
  encodePaymentResponseHeader,
  getHeader,
} from "./headers.js";

export interface HttpAdapterOptions {
  engine: SellerEngine;
  /** Exact raw path that may be served (byte-for-byte). */
  allowlistedPath: string;
  /** Optional request-id / event log sink for tests. */
  onEvent?: (event: AdapterEvent) => void;
}

export type AdapterEvent =
  | { type: "request"; method: string; path: string }
  | { type: "reject_path"; path: string; reason: string }
  | { type: "reject_payment_header"; reason: string }
  | { type: "engine_result"; code: string; httpStatus: number }
  | { type: "stripped_client_auth_headers"; names: string[] };

export interface ListeningServer {
  server: http.Server;
  host: string;
  port: number;
  baseUrl: string;
  close(): Promise<void>;
}

const CLIENT_AUTH_HEADER_NAMES = [
  "authorization",
  "x-api-key",
  "api_key",
  "api-key",
] as const;

export function createSellerHttpServer(opts: HttpAdapterOptions): http.Server {
  const { engine, allowlistedPath, onEvent } = opts;

  return http.createServer((req, res) => {
    void handleRequest(req, res, engine, allowlistedPath, onEvent);
  });
}

export async function listenSellerHttpServer(
  opts: HttpAdapterOptions & { host?: string; port?: number },
): Promise<ListeningServer> {
  const host = opts.host ?? "127.0.0.1";
  const port = opts.port ?? 0;
  const server = createSellerHttpServer(opts);
  await new Promise<void>((resolve, reject) => {
    server.once("error", reject);
    server.listen(port, host, () => resolve());
  });
  const addr = server.address();
  if (!addr || typeof addr === "string") {
    throw new Error("failed to bind HTTP adapter");
  }
  return {
    server,
    host: addr.address,
    port: addr.port,
    baseUrl: `http://${addr.address}:${addr.port}`,
    close: () =>
      new Promise<void>((resolve, reject) => {
        server.close((err) => (err ? reject(err) : resolve()));
      }),
  };
}

async function handleRequest(
  req: http.IncomingMessage,
  res: http.ServerResponse,
  engine: SellerEngine,
  allowlistedPath: string,
  onEvent?: (event: AdapterEvent) => void,
): Promise<void> {
  const method = req.method ?? "GET";
  // Raw path: take the request-target path segment before '?' with no decoding
  // so %xx / dot-segments never match the allowlisted route (spec fingerprint rule).
  const rawTarget = req.url ?? "/";
  const qIndex = rawTarget.indexOf("?");
  const rawPath = qIndex === -1 ? rawTarget : rawTarget.slice(0, qIndex);
  const rawQuery = qIndex === -1 ? "" : rawTarget.slice(qIndex + 1);

  onEvent?.({ type: "request", method, path: rawPath });

  if (rawPath !== allowlistedPath) {
    onEvent?.({ type: "reject_path", path: rawPath, reason: "not_allowlisted" });
    writeJson(res, 404, { code: "not_found", reason: "path_not_allowlisted" });
    return;
  }

  if (method !== "GET") {
    writeJson(res, 405, { code: "method_not_allowed" });
    return;
  }

  const stripped: string[] = [];
  for (const name of CLIENT_AUTH_HEADER_NAMES) {
    if (
      getHeader(req.headers as Record<string, string | string[] | undefined>, name) !==
      undefined
    ) {
      stripped.push(name);
    }
  }
  if (stripped.length > 0) {
    onEvent?.({ type: "stripped_client_auth_headers", names: stripped });
  }

  let bodyByteLength = 0;
  try {
    bodyByteLength = await readBodyByteLength(req, 16_384);
  } catch (e) {
    const reason = e instanceof Error ? e.message : "body_error";
    writeJson(res, 400, { code: "bad_request", reason });
    return;
  }

  let payment: PaymentPayload | null = null;
  const paymentHeader = getHeader(
    req.headers as Record<string, string | string[] | undefined>,
    HEADER_PAYMENT_SIGNATURE,
  );
  if (paymentHeader !== undefined) {
    try {
      payment = decodePaymentSignatureHeader(paymentHeader);
    } catch (e) {
      const reason =
        e instanceof PaymentHeaderError ? e.reason : "payment_signature_invalid";
      onEvent?.({ type: "reject_payment_header", reason });
      writeJson(res, 400, { code: "bad_request", reason });
      return;
    }
  }

  let result: SellerResult;
  try {
    result = await engine.handle({
      method: "GET",
      path: rawPath,
      rawQuery,
      bodyByteLength,
      payment,
    });
  } catch (e) {
    const msg = e instanceof Error ? e.message : "engine_error";
    writeJson(res, 500, { code: "upstream_error", reason: msg });
    return;
  }

  onEvent?.({
    type: "engine_result",
    code: result.code,
    httpStatus: result.httpStatus,
  });
  writeSellerResult(res, result);
}

function writeSellerResult(res: http.ServerResponse, result: SellerResult): void {
  const headers: Record<string, string> = {
    "content-type": "application/json; charset=utf-8",
  };

  if (
    result.retryAfterSeconds !== undefined &&
    (result.code === "payment_submission_unresolved" ||
      result.code === "upstream_error" ||
      result.httpStatus === 503)
  ) {
    headers["retry-after"] = String(result.retryAfterSeconds);
  }

  if (result.code === "payment_required" && result.paymentRequirements) {
    const envelope = buildPaymentRequiredEnvelope(result.paymentRequirements);
    headers[HEADER_PAYMENT_REQUIRED] = encodePaymentRequiredHeader(envelope);
    const body = {
      code: result.code,
      x402Version: envelope.x402Version,
      accepts: envelope.accepts,
    };
    res.writeHead(result.httpStatus, headers);
    res.end(JSON.stringify(body));
    return;
  }

  if (result.code === "ok" && result.body && typeof result.body === "object") {
    const receipt = (result.body as { receipt?: { gatewayTransferId?: string | null } })
      .receipt;
    if (receipt?.gatewayTransferId) {
      headers[HEADER_PAYMENT_RESPONSE] = encodePaymentResponseHeader({
        success: true,
        transaction: receipt.gatewayTransferId,
      });
    }
  }

  if (
    (result.code === "payment_rejected" || result.code === "payment_failed") &&
    result.gatewayErrorReason
  ) {
    headers[HEADER_PAYMENT_RESPONSE] = encodePaymentResponseHeader({
      success: false,
      transaction: "",
      errorReason: result.gatewayErrorReason,
    });
  }

  const extra: Record<string, unknown> = { code: result.code };
  if (result.authKey) extra.authKey = result.authKey;
  if (result.gatewayErrorReason) extra.gatewayErrorReason = result.gatewayErrorReason;
  if (result.body !== undefined) {
    if (typeof result.body === "object" && result.body !== null) {
      Object.assign(extra, result.body);
    } else {
      extra.data = result.body;
    }
  }

  res.writeHead(result.httpStatus, headers);
  res.end(JSON.stringify(extra));
}

function writeJson(
  res: http.ServerResponse,
  status: number,
  body: Record<string, unknown>,
): void {
  res.writeHead(status, { "content-type": "application/json; charset=utf-8" });
  res.end(JSON.stringify(body));
}

function readBodyByteLength(
  req: http.IncomingMessage,
  maxBytes: number,
): Promise<number> {
  return new Promise((resolve, reject) => {
    let total = 0;
    req.on("data", (chunk: Buffer) => {
      total += chunk.length;
      if (total > maxBytes) {
        req.destroy();
        reject(new Error("body_too_large"));
      }
    });
    req.on("end", () => resolve(total));
    req.on("error", (e) => reject(e));
  });
}
