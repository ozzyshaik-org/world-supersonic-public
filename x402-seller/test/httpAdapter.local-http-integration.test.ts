/**
 * local-http-integration + unit-fake Gateway
 *
 * Real node:http adapter + real local upstream test server.
 * Gateway is FakeGatewayClient (synthetic). NOT live Arc / Circle evidence.
 */
import http from "node:http";
import { afterEach, describe, expect, it } from "vitest";
import { SellerEngine } from "../src/seller.js";
import { FakeGatewayClient } from "../src/gateway/fakeGateway.js";
import { MemoryLedger } from "../src/ledger/memoryLedger.js";
import { HttpUpstreamClient } from "../src/http/upstreamHttp.js";
import { TimeoutGatewayClient } from "../src/http/timeoutGateway.js";
import {
  listenSellerHttpServer,
  type AdapterEvent,
  type ListeningServer,
} from "../src/http/server.js";
import {
  HEADER_PAYMENT_REQUIRED,
  HEADER_PAYMENT_SIGNATURE,
  encodePaymentSignatureHeader,
} from "../src/http/headers.js";
import { DEFAULT_PRICE_TABLE } from "../src/priceTable.js";
import {
  FIXTURE_ASSET,
  FIXTURE_NETWORK,
  FIXTURE_PAY_TO,
  FIXTURE_VERIFYING,
  fakeNonce,
  makePayment,
} from "./helpers.js";

const ALLOWLISTED = "/v1/mev/analytics";
const MEV_PRICE = "20000";
const UPSTREAM_API_KEY = "server-side-test-key-not-a-secret";
const CLIENT_SPOOF_KEY = "client-spoofed-key-MUST-NOT-REACH-UPSTREAM";

interface UpstreamHit {
  method: string;
  url: string;
  headers: http.IncomingHttpHeaders;
}

interface UpstreamServer {
  baseUrl: string;
  hits: UpstreamHit[];
  redirectHits: number;
  mode:
    | { kind: "json"; status: number; body: unknown }
    | { kind: "redirect"; status: number; location: string }
    | { kind: "hang"; ms: number }
    | { kind: "sequence"; modes: UpstreamServer["mode"][] };
  sequenceIndex: number;
  close(): Promise<void>;
}

async function startUpstream(): Promise<UpstreamServer> {
  const state: UpstreamServer = {
    baseUrl: "",
    hits: [],
    redirectHits: 0,
    mode: { kind: "json", status: 200, body: { fixture: "FAKE_upstream_mev", ok: true } },
    sequenceIndex: 0,
    close: async () => undefined,
  };

  const redirectServer = http.createServer((_req, res) => {
    state.redirectHits += 1;
    res.writeHead(200, { "content-type": "application/json" });
    res.end(JSON.stringify({ leaked: true }));
  });
  await new Promise<void>((r) => redirectServer.listen(0, "127.0.0.1", () => r()));
  const redirectAddr = redirectServer.address() as { port: number };
  const redirectUrl = `http://127.0.0.1:${redirectAddr.port}/elsewhere`;

  const server = http.createServer(async (req, res) => {
    state.hits.push({
      method: req.method ?? "",
      url: req.url ?? "",
      headers: { ...req.headers },
    });

    let mode = state.mode;
    if (mode.kind === "sequence") {
      mode = mode.modes[state.sequenceIndex] ?? mode.modes[mode.modes.length - 1]!;
      state.sequenceIndex += 1;
    }

    if (mode.kind === "hang") {
      await new Promise((r) => setTimeout(r, mode.ms));
      res.writeHead(200, { "content-type": "application/json" });
      res.end(JSON.stringify({ late: true }));
      return;
    }
    if (mode.kind === "redirect") {
      res.writeHead(mode.status, { Location: mode.location || redirectUrl });
      res.end();
      return;
    }
    res.writeHead(mode.status, { "content-type": "application/json" });
    res.end(JSON.stringify(mode.body));
  });

  await new Promise<void>((r) => server.listen(0, "127.0.0.1", () => r()));
  const addr = server.address() as { port: number };
  state.baseUrl = `http://127.0.0.1:${addr.port}`;
  state.close = () =>
    Promise.all([
      new Promise<void>((res, rej) => server.close((e) => (e ? rej(e) : res()))),
      new Promise<void>((res, rej) => redirectServer.close((e) => (e ? rej(e) : res()))),
    ]).then(() => undefined);
  // stash redirect URL on state via closure for tests
  (state as UpstreamServer & { redirectUrl: string }).redirectUrl = redirectUrl;
  return state;
}

interface Harness {
  upstream: UpstreamServer;
  gateway: FakeGatewayClient;
  adapter: ListeningServer;
  events: AdapterEvent[];
  engine: SellerEngine;
  settleOrder: string[];
}

async function startHarness(opts: {
  settleMode?: ConstructorParameters<typeof FakeGatewayClient>[0];
  unresolvedRetryAfterSeconds?: number;
  settleTimeoutMs?: number;
  upstreamTimeoutMs?: number;
} = {}): Promise<Harness> {
  const upstream = await startUpstream();
  const gateway = new FakeGatewayClient(opts.settleMode ?? { settleMode: { kind: "success" } });
  const settleOrder: string[] = [];
  const orderedGateway: FakeGatewayClient = new Proxy(gateway, {
    get(target, prop, receiver) {
      if (prop === "settle") {
        return async (...args: Parameters<FakeGatewayClient["settle"]>) => {
          settleOrder.push("settle");
          return target.settle(...args);
        };
      }
      return Reflect.get(target, prop, receiver);
    },
  }) as FakeGatewayClient;

  const httpUpstream = new HttpUpstreamClient({
    baseUrl: upstream.baseUrl,
    apiKey: UPSTREAM_API_KEY,
    timeoutMs: opts.upstreamTimeoutMs ?? 1_000,
  });
  const originalFetch = httpUpstream.fetch.bind(httpUpstream);
  httpUpstream.fetch = async (req) => {
    settleOrder.push("upstream");
    return originalFetch(req);
  };

  const timedGateway = new TimeoutGatewayClient(
    orderedGateway,
    opts.settleTimeoutMs ?? 2_000,
  );

  const priceTable = DEFAULT_PRICE_TABLE.filter((e) => e.path === ALLOWLISTED);
  const engine = new SellerEngine({
    gateway: timedGateway,
    upstream: httpUpstream,
    ledger: new MemoryLedger(),
    config: {
      network: FIXTURE_NETWORK,
      asset: FIXTURE_ASSET,
      payTo: FIXTURE_PAY_TO,
      verifyingContract: FIXTURE_VERIFYING,
      priceTable,
      unresolvedRetryAfterSeconds: opts.unresolvedRetryAfterSeconds ?? 7,
      clock: () => new Date(1_800_000_000_000),
      fulfillmentRetryBudget: 2,
      fulfillmentRetryBudgetMs: 60_000,
      attemptCap: 3,
      leaseMs: 1_000,
      leaseWaitMs: 500,
    },
  });

  const events: AdapterEvent[] = [];
  const adapter = await listenSellerHttpServer({
    engine,
    allowlistedPath: ALLOWLISTED,
    host: "127.0.0.1",
    port: 0,
    onEvent: (e) => events.push(e),
  });

  return { upstream, gateway: orderedGateway, adapter, events, engine, settleOrder };
}

const openHarnesses: Harness[] = [];

afterEach(async () => {
  while (openHarnesses.length > 0) {
    const h = openHarnesses.pop()!;
    await h.adapter.close();
    await h.upstream.close();
  }
});

async function boot(opts?: Parameters<typeof startHarness>[0]): Promise<Harness> {
  const h = await startHarness(opts);
  openHarnesses.push(h);
  return h;
}

function mevPayment(nonceSeed = 1) {
  return makePayment({
    value: MEV_PRICE,
    nonce: fakeNonce(nonceSeed),
    acceptedAmount: MEV_PRICE,
  });
}

async function get(
  baseUrl: string,
  pathAndQuery: string,
  headers: Record<string, string> = {},
): Promise<{ status: number; headers: Headers; body: unknown; raw: string }> {
  const res = await fetch(`${baseUrl}${pathAndQuery}`, {
    method: "GET",
    headers,
    redirect: "manual",
  });
  const raw = await res.text();
  let body: unknown = raw;
  try {
    body = raw.length ? JSON.parse(raw) : null;
  } catch {
    /* keep raw */
  }
  return { status: res.status, headers: res.headers, body, raw };
}

describe("local-http-integration + unit-fake Gateway", () => {
  it("unpaid request returns 402 with PAYMENT-REQUIRED header and accepts body", async () => {
    const h = await boot();
    const res = await get(h.adapter.baseUrl, `${ALLOWLISTED}?type=sandwich&block=1`);
    expect(res.status).toBe(402);
    const pr = res.headers.get("payment-required");
    expect(pr).toBeTruthy();
    const decoded = JSON.parse(Buffer.from(pr!, "base64").toString("utf8")) as {
      x402Version: number;
      accepts: Array<{ amount: string; network: string; payTo: string }>;
    };
    expect(decoded.x402Version).toBe(2);
    expect(decoded.accepts).toHaveLength(1);
    expect(decoded.accepts[0]!.amount).toBe(MEV_PRICE);
    expect(decoded.accepts[0]!.network).toBe(FIXTURE_NETWORK);
    expect(decoded.accepts[0]!.payTo).toBe(FIXTURE_PAY_TO);
    const body = res.body as { code: string; x402Version: number; accepts: unknown[] };
    expect(body.code).toBe("payment_required");
    expect(body.x402Version).toBe(2);
    expect(body.accepts).toHaveLength(1);
    expect(h.upstream.hits).toHaveLength(0);
    expect(h.gateway.settleCalls).toHaveLength(0);
  });

  it("paid path settles before upstream and returns 2xx fulfilled body", async () => {
    const h = await boot();
    const payment = mevPayment(10);
    const res = await get(h.adapter.baseUrl, `${ALLOWLISTED}?type=sandwich&block=2`, {
      [HEADER_PAYMENT_SIGNATURE]: encodePaymentSignatureHeader(payment),
    });
    expect(res.status).toBe(200);
    expect(h.gateway.settleCalls).toHaveLength(1);
    expect(h.upstream.hits).toHaveLength(1);
    expect(h.settleOrder).toEqual(["settle", "upstream"]);
    const body = res.body as { code: string; data: { fixture: string }; receipt: { gatewayTransferId: string } };
    expect(body.code).toBe("ok");
    expect(body.data.fixture).toBe("FAKE_upstream_mev");
    expect(body.receipt.gatewayTransferId).toMatch(/^fake-transfer-/);
    expect(h.upstream.hits[0]!.headers["x-api-key"]).toBe(UPSTREAM_API_KEY);
  });

  it("same-binding replay returns stored body without second settle or upstream", async () => {
    const h = await boot();
    const payment = mevPayment(11);
    const headers = {
      [HEADER_PAYMENT_SIGNATURE]: encodePaymentSignatureHeader(payment),
    };
    const first = await get(h.adapter.baseUrl, `${ALLOWLISTED}?type=sandwich&block=3`, headers);
    expect(first.status).toBe(200);
    const settleAfterFirst = h.gateway.settleCalls.length;
    const upstreamAfterFirst = h.upstream.hits.length;
    expect(settleAfterFirst).toBe(1);
    expect(upstreamAfterFirst).toBe(1);

    const second = await get(h.adapter.baseUrl, `${ALLOWLISTED}?type=sandwich&block=3`, headers);
    expect(second.status).toBe(200);
    expect(second.raw).toBe(first.raw);
    expect(h.gateway.settleCalls.length).toBe(settleAfterFirst);
    expect(h.upstream.hits.length).toBe(upstreamAfterFirst);
  });

  it("unresolved settle emits Retry-After with configured seconds", async () => {
    const h = await boot({
      settleMode: { settleMode: { kind: "uncertain", reason: "timeout" } },
      unresolvedRetryAfterSeconds: 9,
    });
    const payment = mevPayment(12);
    const res = await get(h.adapter.baseUrl, `${ALLOWLISTED}?type=sandwich&block=4`, {
      [HEADER_PAYMENT_SIGNATURE]: encodePaymentSignatureHeader(payment),
    });
    expect(res.status).toBe(503);
    expect((res.body as { code: string }).code).toBe("payment_submission_unresolved");
    expect(res.headers.get("retry-after")).toBe("9");
    expect(h.upstream.hits).toHaveLength(0);
  });

  it("upstream 3xx is not followed; redirect target never hit", async () => {
    const h = await boot();
    const redirectUrl = (h.upstream as UpstreamServer & { redirectUrl: string }).redirectUrl;
    h.upstream.mode = { kind: "redirect", status: 302, location: redirectUrl };
    const payment = mevPayment(13);
    const res = await get(h.adapter.baseUrl, `${ALLOWLISTED}?type=sandwich&block=5`, {
      [HEADER_PAYMENT_SIGNATURE]: encodePaymentSignatureHeader(payment),
    });
    // Engine treats non-2xx (including 302) as paid_unfulfilled / 502
    expect(res.status).toBe(502);
    expect((res.body as { code: string }).code).toBe("paid_unfulfilled");
    expect(h.upstream.hits).toHaveLength(1);
    expect(h.upstream.redirectHits).toBe(0);
  });

  it("upstream timeout surfaces as retryable upstream_error with Retry-After", async () => {
    const h = await boot({ upstreamTimeoutMs: 50 });
    h.upstream.mode = { kind: "hang", ms: 500 };
    const payment = mevPayment(14);
    const res = await get(h.adapter.baseUrl, `${ALLOWLISTED}?type=sandwich&block=6`, {
      [HEADER_PAYMENT_SIGNATURE]: encodePaymentSignatureHeader(payment),
    });
    expect(res.status).toBe(503);
    expect((res.body as { code: string }).code).toBe("upstream_error");
    expect(res.headers.get("retry-after")).toBe("7");
  });

  it("settle timeout does not call upstream", async () => {
    const slowGateway = new FakeGatewayClient({ settleMode: { kind: "success" } });
    const original = slowGateway.settle.bind(slowGateway);
    slowGateway.settle = async (...args) => {
      await new Promise((r) => setTimeout(r, 300));
      return original(...args);
    };
    const upstream = await startUpstream();
    const httpUpstream = new HttpUpstreamClient({
      baseUrl: upstream.baseUrl,
      apiKey: UPSTREAM_API_KEY,
      timeoutMs: 1_000,
    });
    const engine = new SellerEngine({
      gateway: new TimeoutGatewayClient(slowGateway, 50),
      upstream: httpUpstream,
      ledger: new MemoryLedger(),
      config: {
        network: FIXTURE_NETWORK,
        asset: FIXTURE_ASSET,
        payTo: FIXTURE_PAY_TO,
        verifyingContract: FIXTURE_VERIFYING,
        priceTable: DEFAULT_PRICE_TABLE.filter((e) => e.path === ALLOWLISTED),
        unresolvedRetryAfterSeconds: 3,
        clock: () => new Date(1_800_000_000_000),
      },
    });
    const adapter = await listenSellerHttpServer({
      engine,
      allowlistedPath: ALLOWLISTED,
      host: "127.0.0.1",
      port: 0,
    });
    openHarnesses.push({
      upstream,
      gateway: slowGateway,
      adapter,
      events: [],
      engine,
      settleOrder: [],
    });

    const payment = mevPayment(15);
    const res = await get(adapter.baseUrl, `${ALLOWLISTED}?type=sandwich&block=7`, {
      [HEADER_PAYMENT_SIGNATURE]: encodePaymentSignatureHeader(payment),
    });
    // Timeout during settle → engine classifies as uncertain / 503 unresolved
    expect(res.status).toBe(503);
    expect(upstream.hits).toHaveLength(0);
  });

  it("direct paid-route bypass attempts are rejected; upstream never hit", async () => {
    const h = await boot();
    const attempts = [
      "/v1/balance?address=0xabc",
      "/v1/mev/analytics/../balance",
      "/%76%31/mev/analytics",
      "/v1/mev/analytics/",
      "/V1/mev/analytics",
    ];
    for (const path of attempts) {
      const res = await get(h.adapter.baseUrl, path, {
        [HEADER_PAYMENT_SIGNATURE]: encodePaymentSignatureHeader(mevPayment(20)),
        "X-API-Key": CLIENT_SPOOF_KEY,
      });
      expect(res.status).toBe(404);
      expect((res.body as { reason: string }).reason).toBe("path_not_allowlisted");
    }
    expect(h.upstream.hits).toHaveLength(0);
    expect(h.gateway.settleCalls).toHaveLength(0);
  });

  it("POST and other methods on allowlisted path are rejected without upstream", async () => {
    const h = await boot();
    const res = await fetch(`${h.adapter.baseUrl}${ALLOWLISTED}?type=x&block=1`, {
      method: "POST",
      headers: {
        [HEADER_PAYMENT_SIGNATURE]: encodePaymentSignatureHeader(mevPayment(21)),
      },
    });
    expect(res.status).toBe(405);
    expect(h.upstream.hits).toHaveLength(0);
    expect(h.gateway.settleCalls).toHaveLength(0);
  });

  it("malformed PAYMENT-SIGNATURE returns 400; upstream and Gateway never hit", async () => {
    const h = await boot();
    const cases = [
      "!!!",
      Buffer.from("{", "utf8").toString("base64"),
      Buffer.from("null", "utf8").toString("base64"),
      Buffer.from("[]", "utf8").toString("base64"),
    ];
    for (const bad of cases) {
      const res = await get(h.adapter.baseUrl, `${ALLOWLISTED}?type=x&block=1`, {
        [HEADER_PAYMENT_SIGNATURE]: bad,
      });
      expect(res.status).toBe(400);
      expect((res.body as { code: string }).code).toBe("bad_request");
    }
    expect(h.upstream.hits).toHaveLength(0);
    expect(h.gateway.settleCalls).toHaveLength(0);
  });

  it("server-side API key reaches upstream; client keys are stripped / ignored", async () => {
    const h = await boot();
    const payment = mevPayment(22);
    const res = await get(h.adapter.baseUrl, `${ALLOWLISTED}?type=sandwich&block=8`, {
      [HEADER_PAYMENT_SIGNATURE]: encodePaymentSignatureHeader(payment),
      "X-API-Key": CLIENT_SPOOF_KEY,
      Authorization: `Bearer ${CLIENT_SPOOF_KEY}`,
      api_key: CLIENT_SPOOF_KEY,
    });
    expect(res.status).toBe(200);
    expect(h.upstream.hits).toHaveLength(1);
    const sent = h.upstream.hits[0]!.headers;
    expect(sent["x-api-key"]).toBe(UPSTREAM_API_KEY);
    expect(sent["x-api-key"]).not.toBe(CLIENT_SPOOF_KEY);
    expect(sent.authorization).toBeUndefined();
    const stripped = h.events.filter((e) => e.type === "stripped_client_auth_headers");
    expect(stripped.length).toBeGreaterThanOrEqual(1);
  });

  it("non-2xx upstream (4xx) becomes paid_unfulfilled without claiming live Arc", async () => {
    const h = await boot();
    h.upstream.mode = { kind: "json", status: 400, body: { error: "FAKE_bad" } };
    const payment = mevPayment(23);
    const res = await get(h.adapter.baseUrl, `${ALLOWLISTED}?type=sandwich&block=9`, {
      [HEADER_PAYMENT_SIGNATURE]: encodePaymentSignatureHeader(payment),
    });
    expect(res.status).toBe(502);
    expect((res.body as { code: string }).code).toBe("paid_unfulfilled");
    expect(h.gateway.settleCalls).toHaveLength(1);
    expect(h.upstream.hits).toHaveLength(1);
  });
});
