import Database from "better-sqlite3";
import { canonicalizeQuery, equalSecret, fingerprint, randomToken, sha256, tokenHash, AgentPassQueryError } from "./crypto.js";
import {
  AgentPassError,
  fail,
  FRESH_ORB_AMR,
  ORB_ACR,
  type AgentPassPolicy,
  type AttemptPurpose,
  type OidcProvider,
  type Operation,
  type OperationInput,
  type OperationState,
  type ProtectedAction,
  type Session,
  type ValidatedIdentity,
} from "./types.js";

interface AccountRow {
  id: string;
  display_name: string;
  world_iss: string | null;
  world_sub: string | null;
}

interface SessionRow {
  id_hash: string;
  account_id: string;
  csrf_token: string;
  expires_at: number;
}

interface OperationRow {
  id: string;
  agent_id: string;
  account_id: string;
  method: string;
  route_path: string;
  canonical_query: string;
  fingerprint: string;
  state: OperationState;
  consequence: "elevated_read";
  created_at: number;
  expires_at: number;
  response_json: string | null;
  error_code: string | null;
}

interface AttemptRow {
  id: string;
  operation_id: string | null;
  session_id_hash: string;
  account_id: string;
  purpose: AttemptPurpose;
  state_value: string;
  nonce_value: string;
  pkce_verifier: string;
  status: "pending" | "redeeming" | "validated" | "failed" | "superseded";
  created_at: number;
  expires_at: number;
  validated_iss: string | null;
  validated_sub: string | null;
  auth_time: number | null;
  acr: string | null;
  amr_json: string | null;
}

const ACTIVE_STATES: OperationState[] = ["proposed", "awaiting_auth", "awaiting_approval", "approved"];
const CANCELLABLE_STATES: OperationState[] = [...ACTIVE_STATES];

export class AgentPassService {
  private readonly db: Database.Database;

  constructor(
    dbPath: string,
    private readonly policy: AgentPassPolicy,
    private readonly oidc: OidcProvider,
    private readonly action: ProtectedAction,
    private readonly now: () => number = () => Date.now(),
  ) {
    this.db = new Database(dbPath);
    this.db.pragma("foreign_keys = ON");
    this.db.pragma("busy_timeout = 5000");
    if (dbPath !== ":memory:") this.db.pragma("journal_mode = WAL");
    this.migrate();
  }

  close(): void {
    this.db.close();
  }

  private migrate(): void {
    this.db.exec(`
      CREATE TABLE IF NOT EXISTS accounts (
        id TEXT PRIMARY KEY,
        display_name TEXT NOT NULL,
        world_iss TEXT,
        world_sub TEXT,
        created_at INTEGER NOT NULL
      );
      CREATE UNIQUE INDEX IF NOT EXISTS accounts_world_unique
        ON accounts(world_iss, world_sub)
        WHERE world_iss IS NOT NULL AND world_sub IS NOT NULL;
      CREATE TABLE IF NOT EXISTS sessions (
        id_hash TEXT PRIMARY KEY,
        account_id TEXT NOT NULL REFERENCES accounts(id),
        csrf_token TEXT NOT NULL,
        created_at INTEGER NOT NULL,
        expires_at INTEGER NOT NULL
      );
      CREATE TABLE IF NOT EXISTS operations (
        id TEXT PRIMARY KEY,
        agent_id TEXT NOT NULL,
        account_id TEXT NOT NULL REFERENCES accounts(id),
        method TEXT NOT NULL CHECK(method = 'GET'),
        route_path TEXT NOT NULL,
        canonical_query TEXT NOT NULL,
        fingerprint TEXT NOT NULL,
        state TEXT NOT NULL,
        consequence TEXT NOT NULL CHECK(consequence = 'elevated_read'),
        created_at INTEGER NOT NULL,
        expires_at INTEGER NOT NULL,
        response_json TEXT,
        error_code TEXT,
        version INTEGER NOT NULL DEFAULT 0
      );
      CREATE TABLE IF NOT EXISTS attempts (
        id TEXT PRIMARY KEY,
        operation_id TEXT REFERENCES operations(id),
        session_id_hash TEXT NOT NULL REFERENCES sessions(id_hash),
        account_id TEXT NOT NULL REFERENCES accounts(id),
        purpose TEXT NOT NULL CHECK(purpose IN ('link', 'checkpoint')),
        state_value TEXT NOT NULL UNIQUE,
        nonce_value TEXT NOT NULL,
        pkce_verifier TEXT NOT NULL,
        status TEXT NOT NULL,
        created_at INTEGER NOT NULL,
        expires_at INTEGER NOT NULL,
        validated_iss TEXT,
        validated_sub TEXT,
        auth_time INTEGER,
        acr TEXT,
        amr_json TEXT
      );
      CREATE INDEX IF NOT EXISTS attempts_operation ON attempts(operation_id, status);
      CREATE TABLE IF NOT EXISTS approvals (
        operation_id TEXT PRIMARY KEY REFERENCES operations(id),
        attempt_id TEXT NOT NULL UNIQUE REFERENCES attempts(id),
        fingerprint TEXT NOT NULL,
        approved_at INTEGER NOT NULL,
        used_at INTEGER
      );
    `);
  }

  /** Demo/test account provisioning. Production deployments use their existing account system. */
  ensureAccount(accountId: string, displayName = accountId): void {
    if (!/^[a-zA-Z0-9_-]{1,80}$/.test(accountId)) fail("invalid_account", 400);
    this.db.prepare("INSERT INTO accounts(id, display_name, created_at) VALUES(?, ?, ?) ON CONFLICT(id) DO NOTHING")
      .run(accountId, displayName, this.now());
  }

  account(accountId: string): { id: string; displayName: string; worldLinked: boolean } {
    const row = this.db.prepare("SELECT * FROM accounts WHERE id = ?").get(accountId) as AccountRow | undefined;
    if (!row) fail("account_not_found", 404);
    return { id: row.id, displayName: row.display_name, worldLinked: Boolean(row.world_iss && row.world_sub) };
  }

  createSession(accountId: string): Session {
    this.account(accountId);
    const token = randomToken();
    const csrfToken = randomToken();
    const expiresAt = this.now() + this.policy.sessionTtlMs;
    this.db.prepare("INSERT INTO sessions(id_hash, account_id, csrf_token, created_at, expires_at) VALUES(?, ?, ?, ?, ?)")
      .run(tokenHash(token), accountId, csrfToken, this.now(), expiresAt);
    return { token, csrfToken, accountId, expiresAt };
  }

  private session(token: string): SessionRow {
    const row = this.db.prepare("SELECT * FROM sessions WHERE id_hash = ?").get(tokenHash(token)) as SessionRow | undefined;
    if (!row || row.expires_at <= this.now()) fail("session_invalid", 401);
    return row;
  }

  private requireCsrf(session: SessionRow, csrfToken: string): void {
    if (!equalSecret(session.csrf_token, csrfToken)) fail("csrf_invalid", 403);
  }

  propose(input: OperationInput): Operation {
    if (input.method !== "GET") fail("method_not_allowed", 400);
    if (!this.policy.allowedRoutes.includes(input.routePath)) fail("route_not_allowlisted", 400);
    this.account(input.accountId);
    let query: string;
    try {
      query = canonicalizeQuery(input.query);
    } catch (error) {
      if (error instanceof AgentPassQueryError) fail(error.code, 400);
      throw error;
    }
    const id = randomToken();
    const createdAt = this.now();
    const digest = fingerprint(input.method, input.routePath, query);
    this.db.prepare(`
      INSERT INTO operations(id, agent_id, account_id, method, route_path, canonical_query,
        fingerprint, state, consequence, created_at, expires_at)
      VALUES(?, ?, ?, ?, ?, ?, ?, 'proposed', 'elevated_read', ?, ?)
    `).run(id, input.agentId, input.accountId, input.method, input.routePath, query, digest, createdAt,
      createdAt + this.policy.operationTtlMs);
    return this.operation(id);
  }

  private operationRow(id: string): OperationRow {
    const row = this.db.prepare("SELECT * FROM operations WHERE id = ?").get(id) as OperationRow | undefined;
    if (!row) fail("operation_not_found", 404);
    return row;
  }

  private mapOperation(row: OperationRow): Operation {
    const base = {
      id: row.id,
      agentId: row.agent_id,
      accountId: row.account_id,
      method: "GET" as const,
      routePath: row.route_path,
      canonicalQuery: row.canonical_query,
      fingerprint: row.fingerprint,
      state: row.state,
      consequence: row.consequence,
      createdAt: row.created_at,
      expiresAt: row.expires_at,
    };
    if (row.response_json !== null) return { ...base, response: JSON.parse(row.response_json) };
    if (row.error_code) return { ...base, errorCode: row.error_code };
    return base;
  }

  operation(id: string): Operation {
    const row = this.operationRow(id);
    this.expireOperation(row);
    return this.mapOperation(this.operationRow(id));
  }

  operationForSession(id: string, sessionToken: string): Operation {
    const session = this.session(sessionToken);
    const operation = this.operation(id);
    if (operation.accountId !== session.account_id) fail("operation_forbidden", 403);
    return operation;
  }

  private expireOperation(row: OperationRow): void {
    if (!ACTIVE_STATES.includes(row.state) || row.expires_at > this.now()) return;
    this.db.transaction(() => {
      const changed = this.db.prepare("UPDATE operations SET state = 'expired', error_code = 'operation_expired', version = version + 1 WHERE id = ? AND state = ?")
        .run(row.id, row.state);
      if (changed.changes === 1) {
        this.db.prepare("UPDATE attempts SET status = 'failed' WHERE operation_id = ? AND status IN ('pending', 'redeeming')").run(row.id);
      }
    })();
  }

  private newAttempt(session: SessionRow, purpose: AttemptPurpose, operationId: string | null) {
    return {
      id: randomToken(),
      state: randomToken(),
      nonce: randomToken(),
      verifier: randomToken(),
      session,
      purpose,
      operationId,
    };
  }

  async startLink(sessionToken: string, csrfToken: string): Promise<string> {
    const session = this.session(sessionToken);
    this.requireCsrf(session, csrfToken);
    const attempt = this.newAttempt(session, "link", null);
    this.db.transaction(() => {
      this.db.prepare("UPDATE attempts SET status = 'superseded' WHERE session_id_hash = ? AND purpose = 'link' AND status = 'pending'")
        .run(session.id_hash);
      this.insertAttempt(attempt);
    })();
    return this.authorizationOrFail(attempt);
  }

  async startAuthentication(operationId: string, sessionToken: string, csrfToken: string): Promise<string> {
    const session = this.session(sessionToken);
    this.requireCsrf(session, csrfToken);
    const operation = this.operation(operationId);
    if (operation.accountId !== session.account_id) fail("operation_forbidden", 403);
    if (!["proposed", "awaiting_auth"].includes(operation.state)) fail("operation_not_authenticatable");
    const account = this.db.prepare("SELECT * FROM accounts WHERE id = ?").get(session.account_id) as AccountRow;
    if (!account.world_iss || !account.world_sub) fail("world_link_required");
    const attempt = this.newAttempt(session, "checkpoint", operationId);
    this.db.transaction(() => {
      const active = this.db.prepare("SELECT COUNT(*) AS n FROM attempts WHERE operation_id = ? AND status = 'redeeming'").get(operationId) as { n: number };
      if (active.n > 0) fail("authentication_in_progress");
      this.db.prepare("UPDATE attempts SET status = 'superseded' WHERE operation_id = ? AND status = 'pending'").run(operationId);
      this.insertAttempt(attempt);
      const result = this.db.prepare("UPDATE operations SET state = 'awaiting_auth', version = version + 1 WHERE id = ? AND state IN ('proposed', 'awaiting_auth')")
        .run(operationId);
      if (result.changes !== 1) fail("operation_not_authenticatable");
    })();
    return this.authorizationOrFail(attempt);
  }

  private insertAttempt(attempt: ReturnType<AgentPassService["newAttempt"]>): void {
    const now = this.now();
    this.db.prepare(`
      INSERT INTO attempts(id, operation_id, session_id_hash, account_id, purpose, state_value,
        nonce_value, pkce_verifier, status, created_at, expires_at)
      VALUES(?, ?, ?, ?, ?, ?, ?, ?, 'pending', ?, ?)
    `).run(attempt.id, attempt.operationId, attempt.session.id_hash, attempt.session.account_id,
      attempt.purpose, attempt.state, attempt.nonce, attempt.verifier, now, now + this.policy.attemptTtlMs);
  }

  private async authorizationOrFail(attempt: ReturnType<AgentPassService["newAttempt"]>): Promise<string> {
    try {
      return await this.oidc.authorizationUrl({
        redirectUri: this.policy.redirectUri,
        state: attempt.state,
        nonce: attempt.nonce,
        codeVerifier: attempt.verifier,
      });
    } catch {
      this.failAttempt(attempt.id, "oidc_dependency_failed");
      fail("oidc_dependency_failed", 503);
    }
  }

  private failAttempt(attemptId: string, code: string): void {
    this.db.transaction(() => {
      const attempt = this.db.prepare("SELECT * FROM attempts WHERE id = ?").get(attemptId) as AttemptRow | undefined;
      if (!attempt || attempt.status === "validated") return;
      this.db.prepare("UPDATE attempts SET status = 'failed' WHERE id = ? AND status IN ('pending', 'redeeming')").run(attemptId);
      if (attempt.operation_id) {
        this.db.prepare("UPDATE operations SET state = 'rejected', error_code = ?, version = version + 1 WHERE id = ? AND state IN ('proposed', 'awaiting_auth')")
          .run(code, attempt.operation_id);
      }
    })();
  }

  async completeCallback(callbackUrl: string, sessionToken: string): Promise<{ purpose: AttemptPurpose; operationId?: string }> {
    const url = new URL(callbackUrl);
    const state = url.searchParams.get("state");
    if (!state) fail("oidc_state_invalid", 403);
    const found = this.db.prepare("SELECT * FROM attempts WHERE state_value = ?").get(state) as AttemptRow | undefined;
    if (!found) fail("oidc_state_invalid", 403);
    const session = this.session(sessionToken);
    if (session.id_hash !== found.session_id_hash || session.account_id !== found.account_id) {
      fail("oidc_session_mismatch", 403);
    }

    this.db.transaction(() => {
      const attempt = this.db.prepare("SELECT * FROM attempts WHERE id = ?").get(found.id) as AttemptRow;
      if (attempt.status !== "pending" || attempt.expires_at <= this.now()) fail("oidc_attempt_replayed_or_expired", 403);
      if (attempt.session_id_hash !== session.id_hash || attempt.account_id !== session.account_id) {
        fail("oidc_session_mismatch", 403);
      }
      if (attempt.operation_id) {
        const operation = this.operationRow(attempt.operation_id);
        this.expireOperation(operation);
        const current = this.operationRow(attempt.operation_id);
        if (current.state !== "awaiting_auth") fail("operation_not_authenticatable");
      }
      const claimed = this.db.prepare("UPDATE attempts SET status = 'redeeming' WHERE id = ? AND status = 'pending'").run(attempt.id);
      if (claimed.changes !== 1) fail("oidc_attempt_replayed_or_expired", 403);
    })();

    try {
      if (url.searchParams.get("iss") !== this.policy.issuer) fail("oidc_issuer_mismatch", 403);
      if (url.searchParams.has("error")) fail("authentication_unsuccessful", 403);
      const identity = await this.oidc.redeem({
        callbackUrl,
        redirectUri: this.policy.redirectUri,
        state: found.state_value,
        nonce: found.nonce_value,
        codeVerifier: found.pkce_verifier,
        maxAgeSeconds: Math.floor(this.policy.authFreshnessMs / 1000),
      });
      this.validateIdentity(identity, found);

      this.db.transaction(() => {
        const attempt = this.db.prepare("SELECT * FROM attempts WHERE id = ?").get(found.id) as AttemptRow;
        if (attempt.status !== "redeeming" || attempt.expires_at <= this.now()) {
          fail("oidc_attempt_replayed_or_expired", 403);
        }
        const currentSession = this.session(sessionToken);
        if (currentSession.id_hash !== attempt.session_id_hash) fail("oidc_session_mismatch", 403);
        const account = this.db.prepare("SELECT * FROM accounts WHERE id = ?").get(attempt.account_id) as AccountRow;

        if (attempt.purpose === "link") {
          if ((account.world_iss && account.world_iss !== identity.iss) ||
              (account.world_sub && account.world_sub !== identity.sub)) {
            fail("world_identity_mismatch", 403);
          }
          const other = this.db.prepare("SELECT id FROM accounts WHERE world_iss = ? AND world_sub = ? AND id <> ?")
            .get(identity.iss, identity.sub, account.id);
          if (other) fail("world_identity_already_linked");
          const linked = this.db.prepare(`
            UPDATE accounts SET world_iss = ?, world_sub = ?
            WHERE id = ? AND ((world_iss IS NULL AND world_sub IS NULL) OR (world_iss = ? AND world_sub = ?))
          `).run(identity.iss, identity.sub, account.id, identity.iss, identity.sub);
          if (linked.changes !== 1) fail("world_identity_mismatch", 403);
        } else {
          if (!attempt.operation_id) fail("oidc_attempt_invalid", 403);
          if (account.world_iss !== identity.iss || account.world_sub !== identity.sub) {
            fail("world_identity_mismatch", 403);
          }
          const operation = this.operationRow(attempt.operation_id);
          if (operation.state !== "awaiting_auth" || operation.expires_at <= this.now() ||
              operation.account_id !== attempt.account_id) {
            fail("operation_not_authenticatable");
          }
          const moved = this.db.prepare("UPDATE operations SET state = 'awaiting_approval', version = version + 1 WHERE id = ? AND state = 'awaiting_auth'")
            .run(operation.id);
          if (moved.changes !== 1) fail("operation_not_authenticatable");
        }

        const validated = this.db.prepare(`
          UPDATE attempts SET status = 'validated', validated_iss = ?, validated_sub = ?,
            auth_time = ?, acr = ?, amr_json = ? WHERE id = ? AND status = 'redeeming'
        `).run(identity.iss, identity.sub, identity.auth_time, ORB_ACR, JSON.stringify(FRESH_ORB_AMR), attempt.id);
        if (validated.changes !== 1) fail("oidc_attempt_replayed_or_expired", 403);
      })();
      return found.operation_id
        ? { purpose: found.purpose, operationId: found.operation_id }
        : { purpose: found.purpose };
    } catch (error) {
      this.failAttempt(found.id, error instanceof AgentPassError ? error.code : "oidc_validation_failed");
      if (error instanceof AgentPassError) throw error;
      fail("oidc_validation_failed", 403);
    }
  }

  private validateIdentity(identity: ValidatedIdentity, attempt: AttemptRow): void {
    const now = this.now();
    if (identity.iss !== this.policy.issuer || !identity.sub) fail("oidc_identity_invalid", 403);
    if (!Number.isFinite(identity.auth_time)) fail("auth_time_invalid", 403);
    if (identity.acr !== ORB_ACR) fail("assurance_acr_rejected", 403);
    if (!Array.isArray(identity.amr) || identity.amr.some((value) => typeof value !== "string")) {
      fail("assurance_amr_rejected", 403);
    }
    const methods = [...identity.amr].sort();
    const required = [...FRESH_ORB_AMR].sort();
    if (methods.length !== required.length || methods.some((value, index) => value !== required[index])) {
      fail("assurance_amr_rejected", 403);
    }
    const authTimeMs = identity.auth_time * 1000;
    if (authTimeMs < attempt.created_at - this.policy.clockToleranceMs ||
        authTimeMs > now + this.policy.clockToleranceMs ||
        now - authTimeMs > this.policy.authFreshnessMs) {
      fail("authentication_not_fresh", 403);
    }
  }

  private validatedAttempt(operationId: string): AttemptRow {
    const row = this.db.prepare(`
      SELECT * FROM attempts WHERE operation_id = ? AND purpose = 'checkpoint' AND status = 'validated'
      ORDER BY created_at DESC LIMIT 1
    `).get(operationId) as AttemptRow | undefined;
    if (!row) fail("validated_attempt_required", 403);
    return row;
  }

  private assertFingerprint(operation: OperationRow, supplied: string): void {
    if (!/^[a-f0-9]{64}$/.test(supplied) || !equalSecret(operation.fingerprint, supplied)) {
      fail("fingerprint_mismatch", 409);
    }
  }

  decide(operationId: string, sessionToken: string, csrfToken: string, suppliedFingerprint: string,
    decision: "approve" | "deny"): Operation {
    const session = this.session(sessionToken);
    this.requireCsrf(session, csrfToken);
    this.db.transaction(() => {
      const operation = this.operationRow(operationId);
      this.expireOperation(operation);
      const current = this.operationRow(operationId);
      if (current.account_id !== session.account_id) fail("operation_forbidden", 403);
      if (current.state !== "awaiting_approval") fail("operation_not_awaiting_approval");
      this.assertFingerprint(current, suppliedFingerprint);
      const attempt = this.validatedAttempt(operationId);
      if (attempt.session_id_hash !== session.id_hash || attempt.account_id !== current.account_id) {
        fail("approval_session_mismatch", 403);
      }
      this.assertExecutionFreshness(attempt);
      if (decision === "approve") {
        this.db.prepare("INSERT INTO approvals(operation_id, attempt_id, fingerprint, approved_at) VALUES(?, ?, ?, ?)")
          .run(operationId, attempt.id, current.fingerprint, this.now());
      }
      const next = decision === "approve" ? "approved" : "denied";
      const changed = this.db.prepare("UPDATE operations SET state = ?, error_code = ?, version = version + 1 WHERE id = ? AND state = 'awaiting_approval'")
        .run(next, decision === "deny" ? "human_denied" : null, operationId);
      if (changed.changes !== 1) fail("operation_not_awaiting_approval");
    })();
    return this.operation(operationId);
  }

  private assertExecutionFreshness(attempt: AttemptRow): void {
    if (attempt.auth_time === null) fail("validated_attempt_required", 403);
    const authTimeMs = attempt.auth_time * 1000;
    const now = this.now();
    if (authTimeMs > now + this.policy.clockToleranceMs || now - authTimeMs > this.policy.authFreshnessMs) {
      fail("authentication_not_fresh", 403);
    }
  }

  cancelByAgent(operationId: string, agentId: string): Operation {
    this.db.transaction(() => {
      const operation = this.operationRow(operationId);
      if (operation.agent_id !== agentId) fail("operation_forbidden", 403);
      this.cancelRow(operation);
    })();
    return this.operation(operationId);
  }

  cancelByHuman(operationId: string, sessionToken: string, csrfToken: string, suppliedFingerprint: string): Operation {
    const session = this.session(sessionToken);
    this.requireCsrf(session, csrfToken);
    this.db.transaction(() => {
      const operation = this.operationRow(operationId);
      if (operation.account_id !== session.account_id) fail("operation_forbidden", 403);
      this.assertFingerprint(operation, suppliedFingerprint);
      const attempt = this.db.prepare("SELECT * FROM attempts WHERE operation_id = ? ORDER BY created_at DESC LIMIT 1")
        .get(operationId) as AttemptRow | undefined;
      if (attempt && attempt.session_id_hash !== session.id_hash) fail("approval_session_mismatch", 403);
      this.cancelRow(operation);
    })();
    return this.operation(operationId);
  }

  private cancelRow(operation: OperationRow): void {
    this.expireOperation(operation);
    const current = this.operationRow(operation.id);
    if (!CANCELLABLE_STATES.includes(current.state)) fail("operation_not_cancellable");
    const changed = this.db.prepare(`UPDATE operations SET state = 'cancelled', error_code = 'cancelled', version = version + 1
      WHERE id = ? AND state IN ('proposed', 'awaiting_auth', 'awaiting_approval', 'approved')`).run(current.id);
    if (changed.changes !== 1) fail("operation_not_cancellable");
    this.db.prepare("UPDATE attempts SET status = 'failed' WHERE operation_id = ? AND status IN ('pending', 'redeeming')").run(current.id);
  }

  async resumeByAgent(operationId: string, agentId: string, suppliedFingerprint: string): Promise<Operation> {
    const reservation = this.reserve(operationId, suppliedFingerprint, { agentId });
    if (reservation.executed) return reservation.executed;
    return this.executeReserved(reservation.reserved!);
  }

  async resumeByHuman(operationId: string, sessionToken: string, csrfToken: string,
    suppliedFingerprint: string): Promise<Operation> {
    const session = this.session(sessionToken);
    this.requireCsrf(session, csrfToken);
    const reservation = this.reserve(operationId, suppliedFingerprint, { session });
    if (reservation.executed) return reservation.executed;
    return this.executeReserved(reservation.reserved!);
  }

  private reserve(operationId: string, suppliedFingerprint: string,
    actor: { agentId?: string; session?: SessionRow }): { reserved?: OperationRow; executed?: Operation } {
    return this.db.transaction(() => {
      const operation = this.operationRow(operationId);
      this.assertFingerprint(operation, suppliedFingerprint);
      if (actor.agentId !== undefined && operation.agent_id !== actor.agentId) fail("operation_forbidden", 403);
      if (actor.session && operation.account_id !== actor.session.account_id) fail("operation_forbidden", 403);
      if (operation.state === "executed") return { executed: this.mapOperation(operation) };
      if (operation.state !== "approved") fail("operation_not_resumable");
      if (operation.expires_at <= this.now()) {
        this.expireOperation(operation);
        fail("operation_expired");
      }

      const approval = this.db.prepare("SELECT * FROM approvals WHERE operation_id = ?").get(operationId) as {
        attempt_id: string; fingerprint: string; used_at: number | null;
      } | undefined;
      if (!approval || approval.used_at !== null || approval.fingerprint !== operation.fingerprint) {
        fail("single_use_approval_unavailable", 403);
      }
      const attempt = this.db.prepare("SELECT * FROM attempts WHERE id = ? AND status = 'validated'")
        .get(approval.attempt_id) as AttemptRow | undefined;
      if (!attempt || attempt.operation_id !== operationId) fail("validated_attempt_required", 403);
      const boundSession = this.db.prepare("SELECT * FROM sessions WHERE id_hash = ?").get(attempt.session_id_hash) as SessionRow | undefined;
      if (!boundSession || boundSession.expires_at <= this.now() || boundSession.account_id !== operation.account_id) {
        fail("session_invalid", 401);
      }
      if (actor.session && attempt.session_id_hash !== actor.session.id_hash) fail("approval_session_mismatch", 403);
      this.assertExecutionFreshness(attempt);
      const account = this.db.prepare("SELECT * FROM accounts WHERE id = ?").get(operation.account_id) as AccountRow;
      if (account.world_iss !== attempt.validated_iss || account.world_sub !== attempt.validated_sub) {
        fail("world_identity_mismatch", 403);
      }

      const consumed = this.db.prepare("UPDATE approvals SET used_at = ? WHERE operation_id = ? AND used_at IS NULL")
        .run(this.now(), operationId);
      if (consumed.changes !== 1) fail("single_use_approval_unavailable", 403);
      const reserved = this.db.prepare("UPDATE operations SET state = 'executing', version = version + 1 WHERE id = ? AND state = 'approved'")
        .run(operationId);
      if (reserved.changes !== 1) fail("operation_not_resumable");
      return { reserved: this.operationRow(operationId) };
    })();
  }

  sessionAccount(sessionToken: string): { account: { id: string; displayName: string; worldLinked: boolean }; csrfToken: string } {
    const session = this.session(sessionToken);
    return { account: this.account(session.account_id), csrfToken: session.csrf_token };
  }

  operationView(operationId: string, sessionToken: string): { operation: Operation; csrfToken: string } {
    const session = this.session(sessionToken);
    const operation = this.operation(operationId);
    if (operation.accountId !== session.account_id) fail("operation_forbidden", 403);
    return { operation, csrfToken: session.csrf_token };
  }

  operationForAgent(operationId: string, agentId: string): Operation {
    const operation = this.operation(operationId);
    if (operation.agentId !== agentId) fail("operation_forbidden", 403);
    return operation;
  }

  private async executeReserved(operation: OperationRow): Promise<Operation> {
    try {
      const response = await this.action.execute(this.mapOperation(operation));
      const changed = this.db.prepare("UPDATE operations SET state = 'executed', response_json = ?, version = version + 1 WHERE id = ? AND state = 'executing'")
        .run(JSON.stringify(response), operation.id);
      if (changed.changes !== 1) throw new Error("execution result commit refused");
      return this.operation(operation.id);
    } catch (error) {
      this.db.prepare("UPDATE operations SET state = 'failed', error_code = 'protected_action_failed', version = version + 1 WHERE id = ? AND state = 'executing'")
        .run(operation.id);
      if (error instanceof AgentPassError) throw error;
      fail("protected_action_failed", 502);
    }
  }
}
