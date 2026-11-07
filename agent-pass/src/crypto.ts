import { createHash, randomBytes, timingSafeEqual } from "node:crypto";

export function randomToken(): string {
  return randomBytes(32).toString("base64url");
}

export function sha256(value: string): string {
  return createHash("sha256").update(value, "utf8").digest("hex");
}

export function tokenHash(token: string): string {
  return sha256(`agent-pass-session\0${token}`);
}

export function equalSecret(left: string, right: string): boolean {
  const a = Buffer.from(left, "utf8");
  const b = Buffer.from(right, "utf8");
  return a.length === b.length && timingSafeEqual(a, b);
}

export function fingerprint(method: string, routePath: string, canonicalQuery: string): string {
  // Empty-body v1 contract. The final separator binds the explicitly empty body.
  return sha256(`${method}\n${routePath}\n${canonicalQuery}\n`);
}

export function canonicalizeQuery(raw = ""): string {
  if (raw.startsWith("?")) raw = raw.slice(1);
  if (/(?:^|&)[^&]*\.eth(?:=|&|$)/i.test(raw) || /\.eth(?:=|%|$)/i.test(raw)) {
    failQuery("ens_names_not_supported");
  }
  const params = new URLSearchParams(raw);
  const entries = [...params.entries()].sort(([ak, av], [bk, bv]) =>
    ak === bk ? av.localeCompare(bv) : ak.localeCompare(bk),
  );
  const canonical = new URLSearchParams();
  for (const [key, value] of entries) {
    if (key.length === 0) failQuery("invalid_query");
    if (/\.eth$/i.test(value)) failQuery("ens_names_not_supported");
    canonical.append(key, value);
  }
  return canonical.toString();
}

function failQuery(code: string): never {
  throw new AgentPassQueryError(code);
}

export class AgentPassQueryError extends Error {
  constructor(public readonly code: string) {
    super(code);
  }
}
