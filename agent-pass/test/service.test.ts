import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { AgentPassError, ORB_ACR } from "../src/types.js";
import { approved, authenticate, harness, link, propose, ISSUER, type Harness } from "./helpers.js";

function hasCode(code: string) {
  return (error: unknown) => error instanceof AgentPassError && error.code === code;
}

async function rejected(promise: Promise<unknown>, code: string): Promise<void> {
  await assert.rejects(promise, hasCode(code));
}

async function ready(h: Harness) {
  await link(h);
  const operation = propose(h);
  const callback = await authenticate(h, operation.id);
  return { operation, callback };
}

describe("Agent Pass service journeys", () => {
  it("runs link → propose → fresh authentication → approval → one protected read", async () => {
    const h = harness();
    try {
      const operation = await approved(h);
      assert.equal(h.action.calls, 0);
      const executed = await h.service.resumeByAgent(operation.id, "agent-1", operation.fingerprint);
      assert.equal(executed.state, "executed");
      assert.equal(h.action.calls, 1);
      assert.deepEqual(executed.response, {
        synthetic: true,
        source: "local-demo-only",
        operationId: operation.id,
        method: "GET",
        routePath: "/v1/transactions",
        query: operation.canonicalQuery,
        result: [],
      });
      const replayed = await h.service.resumeByAgent(operation.id, "agent-1", operation.fingerprint);
      assert.equal(replayed.state, "executed");
      assert.deepEqual(replayed.response, executed.response);
      assert.equal(h.action.calls, 1, "executed resume is read-back, not another upstream call");
    } finally { h.close(); }
  });

  it("denial blocks execution", async () => {
    const h = harness();
    try {
      const { operation } = await ready(h);
      const denied = h.service.decide(operation.id, h.session.token, h.session.csrfToken, operation.fingerprint, "deny");
      assert.equal(denied.state, "denied");
      await rejected(h.service.resumeByAgent(operation.id, "agent-1", operation.fingerprint), "operation_not_resumable");
      assert.equal(h.action.calls, 0);
    } finally { h.close(); }
  });

  it("expiry after approval blocks execution and consumes no action", async () => {
    const h = harness();
    try {
      const operation = await approved(h);
      h.advance(600_001);
      await rejected(h.service.resumeByAgent(operation.id, "agent-1", operation.fingerprint), "operation_expired");
      assert.equal(h.service.operation(operation.id).state, "expired");
      assert.equal(h.action.calls, 0);
    } finally { h.close(); }
  });

  it("wrong-agent and wrong-session cancellation are refused; owning agent can cancel", async () => {
    const h = harness();
    try {
      const operation = await approved(h);
      assert.throws(() => h.service.cancelByAgent(operation.id, "agent-2"), hasCode("operation_forbidden"));
      const other = h.service.createSession("account-2");
      assert.throws(() => h.service.cancelByHuman(operation.id, other.token, other.csrfToken, operation.fingerprint), hasCode("operation_forbidden"));
      assert.equal(h.service.operation(operation.id).state, "approved");
      assert.equal(h.service.cancelByAgent(operation.id, "agent-1").state, "cancelled");
      await rejected(h.service.resumeByAgent(operation.id, "agent-1", operation.fingerprint), "operation_not_resumable");
      assert.equal(h.action.calls, 0);
    } finally { h.close(); }
  });

  it("callback and approval replays are refused", async () => {
    const h = harness();
    try {
      const { operation, callback } = await ready(h);
      await rejected(h.service.completeCallback(callback, h.session.token), "oidc_attempt_replayed_or_expired");
      h.service.decide(operation.id, h.session.token, h.session.csrfToken, operation.fingerprint, "approve");
      assert.throws(() => h.service.decide(operation.id, h.session.token, h.session.csrfToken, operation.fingerprint, "approve"), hasCode("operation_not_awaiting_approval"));
      assert.equal(h.action.calls, 0);
    } finally { h.close(); }
  });

  it("concurrent resumes produce one reservation and one protected call", async () => {
    const h = harness();
    try {
      const operation = await approved(h);
      const results = await Promise.allSettled([
        h.service.resumeByAgent(operation.id, "agent-1", operation.fingerprint),
        h.service.resumeByAgent(operation.id, "agent-1", operation.fingerprint),
      ]);
      assert.equal(results.filter((result) => result.status === "fulfilled").length, 1);
      assert.equal(results.filter((result) => result.status === "rejected").length, 1);
      assert.equal(h.action.calls, 1);
      assert.equal(h.service.operation(operation.id).state, "executed");
    } finally { h.close(); }
  });

  it("cancellation/reservation races have exactly one winner", async () => {
    const cancelFirst = harness();
    try {
      const operation = await approved(cancelFirst);
      cancelFirst.service.cancelByAgent(operation.id, "agent-1");
      await rejected(cancelFirst.service.resumeByAgent(operation.id, "agent-1", operation.fingerprint), "operation_not_resumable");
      assert.equal(cancelFirst.action.calls, 0);
    } finally { cancelFirst.close(); }

    const reserveFirst = harness();
    try {
      const operation = await approved(reserveFirst);
      await reserveFirst.service.resumeByAgent(operation.id, "agent-1", operation.fingerprint);
      assert.throws(() => reserveFirst.service.cancelByAgent(operation.id, "agent-1"), hasCode("operation_not_cancellable"));
      assert.equal(reserveFirst.action.calls, 1);
      assert.equal(reserveFirst.service.operation(operation.id).state, "executed");
    } finally { reserveFirst.close(); }
  });

  it("a changed fingerprint is refused before consuming approval", async () => {
    const h = harness();
    try {
      const operation = await approved(h);
      const wrong = "0".repeat(64);
      await rejected(h.service.resumeByAgent(operation.id, "agent-1", wrong), "fingerprint_mismatch");
      assert.equal(h.service.operation(operation.id).state, "approved");
      await h.service.resumeByAgent(operation.id, "agent-1", operation.fingerprint);
      assert.equal(h.action.calls, 1);
    } finally { h.close(); }
  });

  it("replacement attempts supersede the old callback and succeed only on the new binding", async () => {
    const h = harness();
    try {
      await link(h);
      const operation = propose(h);
      const firstUrl = await h.service.startAuthentication(operation.id, h.session.token, h.session.csrfToken);
      const firstState = h.oidc.stateFromAuthorizationUrl(firstUrl);
      const secondUrl = await h.service.startAuthentication(operation.id, h.session.token, h.session.csrfToken);
      const secondState = h.oidc.stateFromAuthorizationUrl(secondUrl);
      assert.notEqual(firstState, secondState);
      await rejected(h.service.completeCallback(h.oidc.callback(firstState), h.session.token), "oidc_attempt_replayed_or_expired");
      h.oidc.identity.auth_time = h.now() / 1000;
      await h.service.completeCallback(h.oidc.callback(secondState), h.session.token);
      assert.equal(h.service.operation(operation.id).state, "awaiting_approval");
    } finally { h.close(); }
  });

  it("wrong CSRF and cross-session callbacks are refused", async () => {
    const h = harness();
    try {
      await rejected(h.service.startLink(h.session.token, "wrong"), "csrf_invalid");
      await link(h);
      const operation = propose(h);
      const url = await h.service.startAuthentication(operation.id, h.session.token, h.session.csrfToken);
      const other = h.service.createSession("account-1");
      const state = h.oidc.stateFromAuthorizationUrl(url);
      await rejected(h.service.completeCallback(h.oidc.callback(state), other.token), "oidc_session_mismatch");
      assert.equal(h.action.calls, 0);
    } finally { h.close(); }
  });
});

describe("Agent Pass backend-validation failures", () => {
  const cases: Array<{ name: string; code: string; mutate: (h: Harness) => void }> = [
    { name: "wrong response issuer", code: "oidc_issuer_mismatch", mutate: () => {} },
    { name: "wrong linked subject", code: "world_identity_mismatch", mutate: (h) => { h.oidc.identity.sub = "different-subject"; } },
    { name: "wrong assurance class", code: "assurance_acr_rejected", mutate: (h) => { h.oidc.identity.acr = "urn:example:weak"; } },
    { name: "passkey-only profile is not a fresh Orb proof", code: "assurance_amr_rejected", mutate: (h) => { h.oidc.identity.amr = ["pop", "user", "mfa"]; } },
    { name: "stale authentication time", code: "authentication_not_fresh", mutate: (h) => { h.oidc.identity.auth_time = h.now() / 1000 - 601; } },
  ];

  for (const testCase of cases) {
    it(testCase.name, async () => {
      const h = harness();
      try {
        await link(h);
        const operation = propose(h);
        const url = await h.service.startAuthentication(operation.id, h.session.token, h.session.csrfToken);
        const state = h.oidc.stateFromAuthorizationUrl(url);
        testCase.mutate(h);
        const callback = testCase.code === "oidc_issuer_mismatch"
          ? h.oidc.callback(state, "https://attacker.example")
          : h.oidc.callback(state);
        await rejected(h.service.completeCallback(callback, h.session.token), testCase.code);
        assert.equal(h.service.operation(operation.id).state, "rejected");
        await rejected(h.service.resumeByAgent(operation.id, "agent-1", operation.fingerprint), "operation_not_resumable");
        assert.equal(h.action.calls, 0);
      } finally { h.close(); }
    });
  }

  it("OIDC dependency failure leaves the operation rejected and unexecuted", async () => {
    const h = harness();
    try {
      await link(h);
      const operation = propose(h);
      const url = await h.service.startAuthentication(operation.id, h.session.token, h.session.csrfToken);
      const state = h.oidc.stateFromAuthorizationUrl(url);
      h.oidc.redeemError = new Error("injected token endpoint outage");
      await rejected(h.service.completeCallback(h.oidc.callback(state), h.session.token), "oidc_validation_failed");
      assert.equal(h.service.operation(operation.id).state, "rejected");
      assert.equal(h.action.calls, 0);
    } finally { h.close(); }
  });

  it("an OIDC error redirect is unsuccessful and never reaches code redemption", async () => {
    const h = harness();
    try {
      await link(h);
      const operation = propose(h);
      const url = await h.service.startAuthentication(operation.id, h.session.token, h.session.csrfToken);
      const state = h.oidc.stateFromAuthorizationUrl(url);
      const callback = new URL(h.oidc.callback(state));
      callback.searchParams.delete("code");
      callback.searchParams.set("error", "access_denied");
      const calls = h.oidc.redeemCalls;
      await rejected(h.service.completeCallback(callback.href, h.session.token), "authentication_unsuccessful");
      assert.equal(h.oidc.redeemCalls, calls);
      assert.equal(h.service.operation(operation.id).state, "rejected");
      assert.equal(h.action.calls, 0);
    } finally { h.close(); }
  });

  it("literal ENS names are refused at proposal", async () => {
    const h = harness();
    try {
      assert.throws(() => h.service.propose({
        agentId: "agent-1", accountId: "account-1", method: "GET", routePath: "/v1/transactions", query: "address=vitalik.eth",
      }), hasCode("ens_names_not_supported"));
    } finally { h.close(); }
  });

  it("official policy constants remain the fresh Orb-proof profile", () => {
    assert.equal(ORB_ACR, "https://world.org/oidc/acr/orb");
    assert.equal(ISSUER, "https://auth.world.org");
  });
});
