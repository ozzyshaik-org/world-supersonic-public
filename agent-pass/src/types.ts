export const ORB_ACR = "https://world.org/oidc/acr/orb" as const;
export const FRESH_ORB_AMR = ["pop"] as const;

export type OperationState =
  | "proposed"
  | "awaiting_auth"
  | "awaiting_approval"
  | "approved"
  | "executing"
  | "executed"
  | "denied"
  | "cancelled"
  | "expired"
  | "rejected"
  | "failed";

export type AttemptPurpose = "link" | "checkpoint";
export type AttemptStatus = "pending" | "redeeming" | "validated" | "failed" | "superseded";

export interface ValidatedIdentity {
  iss: string;
  sub: string;
  auth_time: number;
  acr?: unknown;
  amr?: unknown;
}

export interface AuthorizationInput {
  redirectUri: string;
  state: string;
  nonce: string;
  codeVerifier: string;
}

export interface RedeemInput extends AuthorizationInput {
  callbackUrl: string;
  maxAgeSeconds: number;
}

/** Only validated claims cross this boundary. ID/access tokens never enter application state. */
export interface OidcProvider {
  authorizationUrl(input: AuthorizationInput): Promise<string>;
  redeem(input: RedeemInput): Promise<ValidatedIdentity>;
}

export interface Operation {
  id: string;
  agentId: string;
  accountId: string;
  method: "GET";
  routePath: string;
  canonicalQuery: string;
  fingerprint: string;
  state: OperationState;
  consequence: "elevated_read";
  createdAt: number;
  expiresAt: number;
  response?: unknown;
  errorCode?: string;
}

export interface OperationInput {
  agentId: string;
  accountId: string;
  method: string;
  routePath: string;
  query?: string;
}

export interface Session {
  token: string;
  csrfToken: string;
  accountId: string;
  expiresAt: number;
}

export interface AgentPassPolicy {
  issuer: string;
  redirectUri: string;
  operationTtlMs: number;
  attemptTtlMs: number;
  authFreshnessMs: number;
  clockToleranceMs: number;
  sessionTtlMs: number;
  allowedRoutes: readonly string[];
}

export interface ProtectedAction {
  execute(operation: Readonly<Operation>): Promise<unknown>;
}

export class AgentPassError extends Error {
  constructor(
    public readonly code: string,
    message: string = code,
    public readonly httpStatus = 409,
  ) {
    super(message);
    this.name = "AgentPassError";
  }
}

export function fail(code: string, httpStatus = 409): never {
  throw new AgentPassError(code, code, httpStatus);
}
