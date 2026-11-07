import type { Operation, ProtectedAction } from "./types.js";

/** Explicitly synthetic local demo result; not World Chain or live-upstream evidence. */
export class SyntheticProtectedAction implements ProtectedAction {
  public calls = 0;

  async execute(operation: Readonly<Operation>): Promise<unknown> {
    this.calls += 1;
    return {
      synthetic: true,
      source: "local-demo-only",
      operationId: operation.id,
      method: operation.method,
      routePath: operation.routePath,
      query: operation.canonicalQuery,
      result: [],
    };
  }
}

/** Server-to-server read. No World ID token or identity claim is forwarded. */
export class FetchProtectedAction implements ProtectedAction {
  constructor(private readonly upstreamOrigin: string) {}

  async execute(operation: Readonly<Operation>): Promise<unknown> {
    const base = new URL(this.upstreamOrigin);
    const suffix = operation.canonicalQuery ? `?${operation.canonicalQuery}` : "";
    const target = new URL(`${operation.routePath}${suffix}`, base);
    if (target.origin !== base.origin || target.pathname !== operation.routePath) {
      throw new Error("protected target escaped trusted upstream");
    }
    const response = await fetch(target, {
      method: "GET",
      headers: { accept: "application/json" },
      redirect: "error",
      signal: AbortSignal.timeout(10_000),
    });
    if (!response.ok) throw new Error(`upstream status ${response.status}`);
    return await response.json();
  }
}
