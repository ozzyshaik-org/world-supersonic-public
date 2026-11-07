import { AgentPassService } from "./service.js";
import { WorldOidcProvider } from "./oidc.js";
import { FetchProtectedAction, SyntheticProtectedAction } from "./action.js";
import { createAgentPassServer } from "./http.js";

function required(name: string): string {
  const value = process.env[name];
  if (!value) throw new Error(`${name} is required`);
  return value;
}

function seconds(name: string, fallback: number): number {
  const raw = process.env[name];
  const value = raw === undefined ? fallback : Number(raw);
  if (!Number.isInteger(value) || value <= 0) throw new Error(`${name} must be a positive integer`);
  return value * 1000;
}

const issuer = process.env.WORLD_ISSUER ?? "https://auth.world.org";
if (new URL(issuer).origin !== "https://auth.world.org") {
  throw new Error("WORLD_ISSUER must be the official https://auth.world.org issuer");
}
const publicOrigin = new URL(required("AGENT_PASS_PUBLIC_ORIGIN")).origin;
const redirectUri = process.env.WORLD_REDIRECT_URI ?? `${publicOrigin}/auth/world/callback`;
if (new URL(redirectUri).origin !== publicOrigin) throw new Error("WORLD_REDIRECT_URI must use AGENT_PASS_PUBLIC_ORIGIN");
const demoEnabled = process.env.AGENT_PASS_ENABLE_DEMO_SESSION === "1";
const demoAccountId = process.env.AGENT_PASS_DEMO_ACCOUNT_ID ?? "demo-account";
const demoAgentId = process.env.AGENT_PASS_DEMO_AGENT_ID ?? "demo-agent";
const demoCredential = process.env.AGENT_PASS_DEMO_AGENT_CREDENTIAL;
if (!demoCredential || demoCredential.length < 24) {
  throw new Error("AGENT_PASS_DEMO_AGENT_CREDENTIAL must be a random value of at least 24 characters");
}

const upstreamOrigin = process.env.AGENT_PASS_UPSTREAM_ORIGIN;
if (!demoEnabled && !upstreamOrigin) throw new Error("AGENT_PASS_UPSTREAM_ORIGIN is required outside demo mode");
const oidc = new WorldOidcProvider({
  issuer,
  clientId: required("WORLD_CLIENT_ID"),
  clientSecret: required("WORLD_CLIENT_SECRET"),
  clientAuthMethod: process.env.WORLD_CLIENT_AUTH_METHOD === "client_secret_post" ? "client_secret_post" : "client_secret_basic",
});
const service = new AgentPassService(
  process.env.AGENT_PASS_DB_PATH ?? "./agent-pass.sqlite",
  {
    issuer,
    redirectUri,
    operationTtlMs: seconds("AGENT_PASS_OPERATION_TTL_SECONDS", 600),
    attemptTtlMs: seconds("AGENT_PASS_ATTEMPT_TTL_SECONDS", 300),
    authFreshnessMs: seconds("AGENT_PASS_AUTH_FRESHNESS_SECONDS", 600),
    clockToleranceMs: seconds("AGENT_PASS_CLOCK_TOLERANCE_SECONDS", 30),
    sessionTtlMs: seconds("AGENT_PASS_SESSION_TTL_SECONDS", 3600),
    allowedRoutes: ["/v1/transactions"],
  },
  oidc,
  upstreamOrigin ? new FetchProtectedAction(upstreamOrigin) : new SyntheticProtectedAction(),
);
if (demoEnabled) service.ensureAccount(demoAccountId, "Synthetic local demo account");
const sessionTtlSeconds = seconds("AGENT_PASS_SESSION_TTL_SECONDS", 3600) / 1000;
const server = createAgentPassServer({
  service,
  publicOrigin,
  agents: { [demoAgentId]: demoCredential },
  ...(demoEnabled ? { demo: { accountId: demoAccountId } } : {}),
  sessionTtlSeconds,
});
const port = Number(process.env.AGENT_PASS_PORT ?? 8097);
const host = process.env.AGENT_PASS_LISTEN_HOST ?? "127.0.0.1";
server.listen(port, host, () => {
  console.log(`Agent Pass listening at ${publicOrigin} (elevated reads only; no Arc payment integration)`);
});
for (const signal of ["SIGINT", "SIGTERM"] as const) {
  process.on(signal, () => server.close(() => {
    service.close();
    process.exit(0);
  }));
}
