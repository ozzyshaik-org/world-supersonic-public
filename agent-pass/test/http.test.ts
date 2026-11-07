import { after, before, describe, it } from "node:test";
import assert from "node:assert/strict";
import net from "node:net";
import type { AddressInfo } from "node:net";
import type { Server } from "node:http";
import { AgentPassService } from "../src/service.js";
import { createAgentPassServer } from "../src/http.js";
import { FakeOidcProvider, ISSUER } from "./helpers.js";
import { SyntheticProtectedAction } from "../src/action.js";

const AGENT_CREDENTIAL = "test-only-agent-credential-32-characters";
let server: Server;
let service: AgentPassService;
let oidc: FakeOidcProvider;
let action: SyntheticProtectedAction;
let base: string;
let cookie = "";

function hidden(html: string, name: string): string {
  const match = new RegExp(`name="${name}" value="([^"]+)"`).exec(html);
  assert.ok(match, `${name} hidden field exists`);
  return match[1]!;
}

async function form(path: string, values: Record<string, string>, expected = 303): Promise<Response> {
  const response = await fetch(`${base}${path}`, {
    method: "POST",
    redirect: "manual",
    headers: {
      cookie,
      origin: base,
      "content-type": "application/x-www-form-urlencoded",
    },
    body: new URLSearchParams(values),
  });
  assert.equal(response.status, expected, await response.text());
  return response;
}

async function callback(authorizationUrl: string): Promise<void> {
  const state = new URL(authorizationUrl).searchParams.get("state")!;
  const target = new URL("/auth/world/callback", base);
  target.searchParams.set("code", "fake-one-time-code");
  target.searchParams.set("state", state);
  target.searchParams.set("iss", ISSUER);
  const response = await fetch(target, { headers: { cookie }, redirect: "manual" });
  assert.equal(response.status, 303, await response.text());
}

async function propose(): Promise<{ id: string; fingerprint: string }> {
  const response = await fetch(`${base}/v1/agent-pass/operations`, {
    method: "POST",
    headers: { authorization: `Bearer ${AGENT_CREDENTIAL}`, "content-type": "application/json" },
    body: JSON.stringify({
      accountId: "account-1",
      method: "GET",
      routePath: "/v1/transactions",
      query: "address=0x0000000000000000000000000000000000000001&limit=1",
    }),
  });
  const raw = await response.text();
  assert.equal(response.status, 201, raw);
  return JSON.parse(raw).operation;
}

async function authenticate(operationId: string): Promise<void> {
  const view = await fetch(`${base}/operations/${operationId}`, { headers: { cookie } });
  const html = await view.text();
  const csrf = hidden(html, "csrf");
  const digest = hidden(html, "fingerprint");
  const started = await form(`/operations/${operationId}/auth/start`, { csrf, fingerprint: digest });
  await callback(started.headers.get("location")!);
}

before(async () => {
  const probe = net.createServer();
  await new Promise<void>((resolve) => probe.listen(0, "127.0.0.1", resolve));
  const port = (probe.address() as AddressInfo).port;
  await new Promise<void>((resolve) => probe.close(() => resolve()));
  base = `http://127.0.0.1:${port}`;
  oidc = new FakeOidcProvider();
  oidc.identity.auth_time = Date.now() / 1000;
  action = new SyntheticProtectedAction();
  service = new AgentPassService(":memory:", {
    issuer: ISSUER,
    redirectUri: `${base}/auth/world/callback`,
    operationTtlMs: 600_000,
    attemptTtlMs: 300_000,
    authFreshnessMs: 600_000,
    clockToleranceMs: 30_000,
    sessionTtlMs: 3_600_000,
    allowedRoutes: ["/v1/transactions"],
  }, oidc, action);
  service.ensureAccount("account-1", "Local integration account");
  server = createAgentPassServer({
    service,
    publicOrigin: base,
    agents: { "agent-1": AGENT_CREDENTIAL },
    demo: { accountId: "account-1" },
    sessionTtlSeconds: 3600,
  });
  await new Promise<void>((resolve) => server.listen(port, "127.0.0.1", resolve));
});

after(async () => {
  await new Promise<void>((resolve) => server.close(() => resolve()));
  service.close();
});

describe("real local HTTP integration with fake OIDC", () => {
  it("sets a protected session cookie and rejects a cross-origin state change", async () => {
    const crossOrigin = await fetch(`${base}/demo/session`, {
      method: "POST", headers: { origin: "https://attacker.example" }, redirect: "manual",
    });
    assert.equal(crossOrigin.status, 403);
    const response = await fetch(`${base}/demo/session`, {
      method: "POST", headers: { origin: base }, redirect: "manual",
    });
    assert.equal(response.status, 303);
    cookie = response.headers.get("set-cookie")!.split(";", 1)[0]!;
    const attributes = response.headers.get("set-cookie")!;
    assert.match(attributes, /HttpOnly/);
    assert.match(attributes, /Secure/);
    assert.match(attributes, /SameSite=Lax/);
    const account = await fetch(`${base}/account`, { headers: { cookie } });
    assert.equal(account.status, 200);
    assert.match(await account.text(), /No World ID is linked/);
  });

  it("completes link, checkpoint, approval and one execution over HTTP", async () => {
    const account = await (await fetch(`${base}/account`, { headers: { cookie } })).text();
    const link = await form("/account/link/start", { csrf: hidden(account, "csrf") });
    const authorization = link.headers.get("location")!;
    assert.equal(new URL(authorization).searchParams.get("max_age"), "0");
    await callback(authorization);
    assert.equal(service.account("account-1").worldLinked, true);

    const operation = await propose();
    assert.equal(action.calls, 0);
    await authenticate(operation.id);
    let view = await (await fetch(`${base}/operations/${operation.id}`, { headers: { cookie } })).text();
    assert.match(view, /awaiting_approval/);
    const csrf = hidden(view, "csrf");
    const digest = hidden(view, "fingerprint");
    await form(`/operations/${operation.id}/decision`, { csrf, fingerprint: digest, decision: "approve" });
    view = await (await fetch(`${base}/operations/${operation.id}`, { headers: { cookie } })).text();
    await form(`/operations/${operation.id}/resume`, { csrf, fingerprint: digest });
    view = await (await fetch(`${base}/operations/${operation.id}`, { headers: { cookie } })).text();
    assert.match(view, /executed/);
    assert.match(view, /local-demo-only/);
    assert.equal(action.calls, 1);

    await form(`/operations/${operation.id}/resume`, { csrf, fingerprint: digest });
    assert.equal(action.calls, 1, "HTTP replay does not repeat the protected read");
  });

  it("denies a second request over HTTP without executing it", async () => {
    const operation = await propose();
    await authenticate(operation.id);
    const view = await (await fetch(`${base}/operations/${operation.id}`, { headers: { cookie } })).text();
    await form(`/operations/${operation.id}/decision`, {
      csrf: hidden(view, "csrf"), fingerprint: hidden(view, "fingerprint"), decision: "deny",
    });
    const afterDenial = await (await fetch(`${base}/operations/${operation.id}`, { headers: { cookie } })).text();
    assert.match(afterDenial, /denied/);
    await form(`/operations/${operation.id}/resume`, {
      csrf: hidden(view, "csrf"), fingerprint: hidden(view, "fingerprint"),
    }, 409);
    assert.equal(action.calls, 1, "only the earlier approved request ran");
  });

  it("rejects bad agent credentials and never reflects untrusted Host into approval URLs", async () => {
    const unauthorized = await fetch(`${base}/v1/agent-pass/operations`, {
      method: "POST",
      headers: { authorization: "Bearer wrong", "content-type": "application/json", host: "attacker.example" },
      body: JSON.stringify({ accountId: "account-1", method: "GET", routePath: "/v1/transactions" }),
    });
    assert.equal(unauthorized.status, 401);
    const operation = await propose();
    const response = await fetch(`${base}/v1/agent-pass/operations/${operation.id}`, {
      headers: { authorization: `Bearer ${AGENT_CREDENTIAL}`, host: "attacker.example" },
    });
    assert.equal(response.status, 200);
    assert.equal(response.url.startsWith(base), true);
  });
});
