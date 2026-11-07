import { AgentPassService } from "../src/service.js";
import { SyntheticProtectedAction } from "../src/action.js";
import {
  ORB_ACR,
  type AuthorizationInput,
  type OidcProvider,
  type RedeemInput,
  type Session,
  type ValidatedIdentity,
} from "../src/types.js";

export const ISSUER = "https://auth.world.org";
export const REDIRECT_URI = "http://127.0.0.1:8097/auth/world/callback";
export const PUBLIC_ORIGIN = "http://127.0.0.1:8097";

export class FakeOidcProvider implements OidcProvider {
  readonly attempts = new Map<string, AuthorizationInput>();
  identity: ValidatedIdentity = {
    iss: ISSUER,
    sub: "pairwise-subject-1",
    auth_time: 1,
    acr: ORB_ACR,
    amr: ["pop"],
  };
  redeemError?: Error;
  redeemCalls = 0;

  async authorizationUrl(input: AuthorizationInput): Promise<string> {
    this.attempts.set(input.state, input);
    const url = new URL("/api/v1/authorize", ISSUER);
    url.searchParams.set("state", input.state);
    url.searchParams.set("nonce", input.nonce);
    url.searchParams.set("code_challenge_method", "S256");
    url.searchParams.set("max_age", "0");
    return url.href;
  }

  async redeem(input: RedeemInput): Promise<ValidatedIdentity> {
    this.redeemCalls += 1;
    const saved = this.attempts.get(input.state);
    if (!saved || saved.nonce !== input.nonce || saved.codeVerifier !== input.codeVerifier) {
      throw new Error("fake OIDC binding mismatch");
    }
    if (this.redeemError) throw this.redeemError;
    return this.identity;
  }

  callback(state: string, issuer = ISSUER): string {
    const url = new URL(REDIRECT_URI);
    url.searchParams.set("code", "fake-one-time-code");
    url.searchParams.set("state", state);
    url.searchParams.set("iss", issuer);
    return url.href;
  }

  stateFromAuthorizationUrl(url: string): string {
    return new URL(url).searchParams.get("state")!;
  }
}

export interface Harness {
  service: AgentPassService;
  oidc: FakeOidcProvider;
  action: SyntheticProtectedAction;
  session: Session;
  now: () => number;
  advance: (milliseconds: number) => void;
  close: () => void;
}

export function harness(): Harness {
  let now = 1_800_000_000_000;
  const oidc = new FakeOidcProvider();
  const action = new SyntheticProtectedAction();
  const service = new AgentPassService(":memory:", {
    issuer: ISSUER,
    redirectUri: REDIRECT_URI,
    operationTtlMs: 600_000,
    attemptTtlMs: 300_000,
    authFreshnessMs: 600_000,
    clockToleranceMs: 30_000,
    sessionTtlMs: 3_600_000,
    allowedRoutes: ["/v1/transactions"],
  }, oidc, action, () => now);
  service.ensureAccount("account-1", "Test account");
  service.ensureAccount("account-2", "Other account");
  const session = service.createSession("account-1");
  oidc.identity.auth_time = now / 1000;
  return {
    service, oidc, action, session,
    now: () => now,
    advance: (milliseconds) => { now += milliseconds; },
    close: () => service.close(),
  };
}

export async function link(h: Harness, session = h.session): Promise<string> {
  h.oidc.identity.auth_time = h.now() / 1000;
  const authorization = await h.service.startLink(session.token, session.csrfToken);
  const state = h.oidc.stateFromAuthorizationUrl(authorization);
  const callback = h.oidc.callback(state);
  await h.service.completeCallback(callback, session.token);
  return callback;
}

export function propose(h: Harness) {
  return h.service.propose({
    agentId: "agent-1",
    accountId: "account-1",
    method: "GET",
    routePath: "/v1/transactions",
    query: "limit=1&address=0x0000000000000000000000000000000000000001",
  });
}

export async function authenticate(h: Harness, operationId: string, session = h.session): Promise<string> {
  h.oidc.identity.auth_time = h.now() / 1000;
  const authorization = await h.service.startAuthentication(operationId, session.token, session.csrfToken);
  const state = h.oidc.stateFromAuthorizationUrl(authorization);
  const callback = h.oidc.callback(state);
  await h.service.completeCallback(callback, session.token);
  return callback;
}

export async function approved(h: Harness) {
  await link(h);
  const operation = propose(h);
  await authenticate(h, operation.id);
  h.service.decide(operation.id, h.session.token, h.session.csrfToken, operation.fingerprint, "approve");
  return h.service.operation(operation.id);
}
