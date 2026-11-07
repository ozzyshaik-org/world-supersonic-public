import { createServer, type IncomingMessage, type Server, type ServerResponse } from "node:http";
import { AgentPassError, fail, type OperationInput } from "./types.js";
import { equalSecret } from "./crypto.js";
import type { AgentPassService } from "./service.js";
import { accountPage, demoPage, errorPage, operationPage, page } from "./page.js";

export interface HttpOptions {
  service: AgentPassService;
  publicOrigin: string;
  agents: Readonly<Record<string, string>>;
  demo?: { accountId: string };
  sessionTtlSeconds: number;
}

const JSON_LIMIT = 16_384;

export function createAgentPassServer(options: HttpOptions): Server {
  const publicOrigin = new URL(options.publicOrigin).origin;
  return createServer(async (request, response) => {
    setSecurityHeaders(response);
    try {
      const url = new URL(request.url ?? "/", publicOrigin);
      const method = request.method ?? "GET";
      const sessionToken = cookies(request).agent_pass_session;

      if (method === "GET" && url.pathname === "/") {
        redirect(response, "/account");
        return;
      }
      if (options.demo && method === "GET" && url.pathname === "/demo") {
        html(response, 200, demoPage(options.demo.accountId));
        return;
      }
      if (options.demo && method === "POST" && url.pathname === "/demo/session") {
        browserPostGuard(request, publicOrigin);
        const session = options.service.createSession(options.demo.accountId);
        response.setHeader("Set-Cookie", sessionCookie(session.token, options.sessionTtlSeconds));
        redirect(response, "/account");
        return;
      }
      if (method === "GET" && url.pathname === "/account") {
        if (!sessionToken) {
          if (options.demo) redirect(response, "/demo");
          else fail("session_invalid", 401);
          return;
        }
        const view = options.service.sessionAccount(sessionToken);
        html(response, 200, accountPage(view.account, view.csrfToken));
        return;
      }
      if (method === "POST" && url.pathname === "/account/link/start") {
        browserPostGuard(request, publicOrigin);
        const form = await body(request);
        const location = await options.service.startLink(requiredSession(sessionToken), text(form, "csrf"));
        redirect(response, location);
        return;
      }
      if (method === "GET" && url.pathname === "/auth/world/callback") {
        const callbackUrl = new URL(`${url.pathname}${url.search}`, publicOrigin).href;
        const result = await options.service.completeCallback(callbackUrl, requiredSession(sessionToken));
        redirect(response, result.operationId ? `/operations/${result.operationId}` : "/account");
        return;
      }
      if (method === "GET" && url.pathname === "/operations/find") {
        const id = url.searchParams.get("id");
        if (!id) fail("operation_not_found", 404);
        redirect(response, `/operations/${encodeURIComponent(id)}`);
        return;
      }

      const browserMatch = /^\/operations\/([A-Za-z0-9_-]+)(?:\/(auth\/start|decision|resume|cancel))?$/.exec(url.pathname);
      if (browserMatch) {
        const id = browserMatch[1]!;
        const action = browserMatch[2];
        if (method === "GET" && !action) {
          const view = options.service.operationView(id, requiredSession(sessionToken));
          html(response, 200, operationPage(view.operation, view.csrfToken));
          return;
        }
        if (method === "POST" && action) {
          browserPostGuard(request, publicOrigin);
          const form = await body(request);
          const csrf = text(form, "csrf");
          const token = requiredSession(sessionToken);
          if (action === "auth/start") {
            const location = await options.service.startAuthentication(id, token, csrf);
            redirect(response, location);
            return;
          }
          const digest = text(form, "fingerprint");
          if (action === "decision") {
            const decision = text(form, "decision");
            if (decision !== "approve" && decision !== "deny") fail("invalid_decision", 400);
            await Promise.resolve(options.service.decide(id, token, csrf, digest, decision));
          } else if (action === "cancel") {
            options.service.cancelByHuman(id, token, csrf, digest);
          } else {
            await options.service.resumeByHuman(id, token, csrf, digest);
          }
          redirect(response, `/operations/${id}`);
          return;
        }
      }

      if (url.pathname === "/v1/agent-pass/operations" && method === "POST") {
        const agentId = authenticateAgent(request, options.agents);
        const input = await body(request) as Record<string, unknown>;
        const operationInput: OperationInput = {
          agentId,
          accountId: requiredString(input.accountId, "accountId"),
          method: requiredString(input.method, "method"),
          routePath: requiredString(input.routePath, "routePath"),
          ...(input.query === undefined ? {} : { query: requiredString(input.query, "query") }),
        };
        const operation = options.service.propose(operationInput);
        json(response, 201, { operation, approvalUrl: `${publicOrigin}/operations/${operation.id}` });
        return;
      }
      const apiMatch = /^\/v1\/agent-pass\/operations\/([A-Za-z0-9_-]+)(?:\/(resume|cancel))?$/.exec(url.pathname);
      if (apiMatch) {
        const agentId = authenticateAgent(request, options.agents);
        const id = apiMatch[1]!;
        const action = apiMatch[2];
        if (method === "GET" && !action) {
          const operation = options.service.operationForAgent(id, agentId);
          json(response, 200, { operation });
          return;
        }
        if (method === "POST" && action === "cancel") {
          json(response, 200, { operation: options.service.cancelByAgent(id, agentId) });
          return;
        }
        if (method === "POST" && action === "resume") {
          const input = await body(request) as Record<string, unknown>;
          const operation = await options.service.resumeByAgent(id, agentId, requiredString(input.fingerprint, "fingerprint"));
          json(response, 200, { operation });
          return;
        }
      }
      fail("not_found", 404);
    } catch (error) {
      const code = error instanceof AgentPassError ? error.code : "internal_error";
      const status = error instanceof AgentPassError ? error.httpStatus : 500;
      if ((request.url ?? "").startsWith("/v1/")) json(response, status, { code });
      else html(response, status, errorPage(code));
    }
  });
}

function setSecurityHeaders(response: ServerResponse): void {
  response.setHeader("Cache-Control", "no-store");
  response.setHeader("X-Content-Type-Options", "nosniff");
  response.setHeader("Referrer-Policy", "no-referrer");
  response.setHeader("Content-Security-Policy", "default-src 'self'; style-src 'unsafe-inline'; script-src 'none'; img-src 'self'; base-uri 'none'; frame-ancestors 'none'; form-action 'self'");
}

function browserPostGuard(request: IncomingMessage, publicOrigin: string): void {
  const origin = request.headers.origin;
  if (origin && origin !== publicOrigin) fail("origin_forbidden", 403);
  if (request.headers["sec-fetch-site"] === "cross-site") fail("origin_forbidden", 403);
}

function authenticateAgent(request: IncomingMessage, agents: Readonly<Record<string, string>>): string {
  const header = request.headers.authorization;
  if (!header?.startsWith("Bearer ")) fail("agent_unauthorized", 401);
  const credential = header.slice(7);
  for (const [id, expected] of Object.entries(agents)) {
    if (equalSecret(credential, expected)) return id;
  }
  fail("agent_unauthorized", 401);
}

async function body(request: IncomingMessage): Promise<URLSearchParams | Record<string, unknown>> {
  const chunks: Buffer[] = [];
  let size = 0;
  for await (const chunk of request) {
    const buffer = Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk);
    size += buffer.length;
    if (size > JSON_LIMIT) fail("body_too_large", 413);
    chunks.push(buffer);
  }
  const raw = Buffer.concat(chunks).toString("utf8");
  if (request.headers["content-type"]?.split(";", 1)[0] === "application/json") {
    try {
      const parsed = JSON.parse(raw || "{}");
      if (!parsed || Array.isArray(parsed) || typeof parsed !== "object") fail("invalid_json", 400);
      return parsed;
    } catch (error) {
      if (error instanceof AgentPassError) throw error;
      fail("invalid_json", 400);
    }
  }
  return new URLSearchParams(raw);
}

function text(form: URLSearchParams | Record<string, unknown>, key: string): string {
  const value = form instanceof URLSearchParams ? form.get(key) : form[key];
  if (typeof value !== "string" || value.length === 0) fail(`missing_${key}`, 400);
  return value;
}

function requiredString(value: unknown, name: string): string {
  if (typeof value !== "string" || value.length === 0 || value.length > 4096) fail(`invalid_${name}`, 400);
  return value;
}

function requiredSession(token: string | undefined): string {
  if (!token) fail("session_invalid", 401);
  return token;
}

function cookies(request: IncomingMessage): Record<string, string> {
  const result: Record<string, string> = {};
  for (const part of (request.headers.cookie ?? "").split(";")) {
    const index = part.indexOf("=");
    if (index > 0) result[part.slice(0, index).trim()] = decodeURIComponent(part.slice(index + 1).trim());
  }
  return result;
}

function sessionCookie(token: string, maxAge: number): string {
  return `agent_pass_session=${encodeURIComponent(token)}; Path=/; HttpOnly; Secure; SameSite=Lax; Max-Age=${maxAge}`;
}

function json(response: ServerResponse, status: number, value: unknown): void {
  response.writeHead(status, { "content-type": "application/json; charset=utf-8" });
  response.end(JSON.stringify(value));
}

function html(response: ServerResponse, status: number, value: string): void {
  response.writeHead(status, { "content-type": "text/html; charset=utf-8" });
  response.end(value);
}

function redirect(response: ServerResponse, location: string): void {
  response.writeHead(303, { location });
  response.end();
}

export function notFoundPage(): string {
  return page("Not found", "<h1>Not found</h1>");
}
