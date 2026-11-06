/**
 * Request fingerprint v1 — docs/hackathon/01-x402-arc-gateway.md
 * Canonical string: five lines joined by 0x0A, no trailing LF.
 */

import { createHash } from "node:crypto";

export const FINGERPRINT_VERSION_LABEL = "supersonic-x402-fp-v1";

export type FingerprintOk = {
  ok: true;
  fingerprint: string;
  canonicalQuery: string;
  canonicalString: string;
};

export type FingerprintErr = {
  ok: false;
  httpStatus: 400 | 404 | 405;
  reason: string;
};

export type FingerprintResult = FingerprintOk | FingerprintErr;

const UNRESERVED = /[A-Za-z0-9\-._~]/;

function encodeRfc3986Byte(byte: number): string {
  const ch = String.fromCharCode(byte);
  if (UNRESERVED.test(ch)) return ch;
  return `%${byte.toString(16).toUpperCase().padStart(2, "0")}`;
}

function encodeComponent(decodedUtf8: string): string {
  const buf = Buffer.from(decodedUtf8, "utf8");
  let out = "";
  for (const b of buf) out += encodeRfc3986Byte(b);
  return out;
}

function percentDecodeOnce(raw: string): { ok: true; value: string } | { ok: false; reason: string } {
  let i = 0;
  const bytes: number[] = [];
  while (i < raw.length) {
    const c = raw[i]!;
    if (c === "%") {
      if (i + 2 >= raw.length) return { ok: false, reason: "bad percent escape" };
      const hex = raw.slice(i + 1, i + 3);
      if (!/^[0-9a-fA-F]{2}$/.test(hex)) return { ok: false, reason: "bad percent escape" };
      bytes.push(parseInt(hex, 16));
      i += 3;
      continue;
    }
    // '+' is a literal plus, not a space
    bytes.push(raw.charCodeAt(i));
    i += 1;
  }
  const buf = Buffer.from(bytes);
  // Valid UTF-8 check
  const asString = buf.toString("utf8");
  if (!Buffer.from(asString, "utf8").equals(buf)) {
    return { ok: false, reason: "not valid UTF-8" };
  }
  // Reject overlong / invalid sequences that Node may replace — compare round-trip
  if (Buffer.byteLength(asString, "utf8") !== buf.length) {
    return { ok: false, reason: "not valid UTF-8" };
  }
  // Detect replacement characters introduced from invalid sequences
  for (let j = 0; j < buf.length; ) {
    const b = buf[j]!;
    if (b <= 0x7f) {
      j += 1;
      continue;
    }
    let need = 0;
    if ((b & 0xe0) === 0xc0) need = 1;
    else if ((b & 0xf0) === 0xe0) need = 2;
    else if ((b & 0xf8) === 0xf0) need = 3;
    else return { ok: false, reason: "not valid UTF-8" };
    if (j + need >= buf.length) return { ok: false, reason: "not valid UTF-8" };
    for (let k = 1; k <= need; k++) {
      if ((buf[j + k]! & 0xc0) !== 0x80) return { ok: false, reason: "not valid UTF-8" };
    }
    // Reject overlong encodings and surrogates / out of range
    if (need === 1 && b < 0xc2) return { ok: false, reason: "not valid UTF-8" };
    if (need === 2) {
      const cp = ((b & 0x0f) << 12) | ((buf[j + 1]! & 0x3f) << 6) | (buf[j + 2]! & 0x3f);
      if (cp < 0x800 || (cp >= 0xd800 && cp <= 0xdfff)) {
        return { ok: false, reason: "not valid UTF-8" };
      }
    }
    if (need === 3) {
      const cp =
        ((b & 0x07) << 18) |
        ((buf[j + 1]! & 0x3f) << 12) |
        ((buf[j + 2]! & 0x3f) << 6) |
        (buf[j + 3]! & 0x3f);
      if (cp < 0x10000 || cp > 0x10ffff) return { ok: false, reason: "not valid UTF-8" };
    }
    j += 1 + need;
  }
  return { ok: true, value: asString };
}

function isAsciiNoSpaceControlHash(s: string): boolean {
  for (let i = 0; i < s.length; i++) {
    const code = s.charCodeAt(i);
    if (code > 0x7f || code <= 0x20 || code === 0x7f || code === 0x23) return false;
  }
  return true;
}

export function computeFingerprintV1(args: {
  method: string;
  path: string;
  rawQuery: string;
  bodyByteLength: number;
  allowedQueryParams: ReadonlySet<string> | readonly string[];
  /** When path is not in the price table, pass false to get 404. */
  pathAllowed: boolean;
}): FingerprintResult {
  const { method, path, rawQuery, bodyByteLength, pathAllowed } = args;
  const allowed =
    args.allowedQueryParams instanceof Set
      ? args.allowedQueryParams
      : new Set(args.allowedQueryParams);

  if (method !== "GET") {
    return { ok: false, httpStatus: 405, reason: "method outside the committed paid flow" };
  }
  if (bodyByteLength !== 0) {
    return { ok: false, httpStatus: 400, reason: "body not empty" };
  }
  if (!pathAllowed) {
    return { ok: false, httpStatus: 404, reason: "no route matches the path" };
  }
  if (!isAsciiNoSpaceControlHash(rawQuery)) {
    return { ok: false, httpStatus: 400, reason: "query has non-ASCII, space, control or #" };
  }

  const pairs: Array<{ name: string; value: string; encName: string; encValue: string }> = [];
  if (rawQuery.length > 0) {
    const segments = rawQuery.split("&");
    for (const segment of segments) {
      if (segment.length === 0) {
        return { ok: false, httpStatus: 400, reason: "empty segment" };
      }
      const eq = segment.indexOf("=");
      if (eq < 0) {
        return { ok: false, httpStatus: 400, reason: "segment without =" };
      }
      const rawName = segment.slice(0, eq);
      const rawValue = segment.slice(eq + 1);
      if (rawName.length === 0) {
        return { ok: false, httpStatus: 400, reason: "empty name" };
      }
      const nameDec = percentDecodeOnce(rawName);
      if (!nameDec.ok) return { ok: false, httpStatus: 400, reason: nameDec.reason };
      const valueDec = percentDecodeOnce(rawValue);
      if (!valueDec.ok) return { ok: false, httpStatus: 400, reason: valueDec.reason };
      if (!allowed.has(nameDec.value)) {
        return { ok: false, httpStatus: 400, reason: "undocumented parameter" };
      }
      const encName = encodeComponent(nameDec.value);
      const encValue = encodeComponent(valueDec.value);
      pairs.push({
        name: nameDec.value,
        value: valueDec.value,
        encName,
        encValue,
      });
    }
    const seen = new Set<string>();
    for (const p of pairs) {
      if (seen.has(p.name)) {
        return { ok: false, httpStatus: 400, reason: "duplicate name" };
      }
      seen.add(p.name);
    }
  }

  pairs.sort((a, b) => {
    const aa = Buffer.from(a.encName, "utf8");
    const bb = Buffer.from(b.encName, "utf8");
    const n = Math.min(aa.length, bb.length);
    for (let i = 0; i < n; i++) {
      if (aa[i]! !== bb[i]!) return aa[i]! - bb[i]!;
    }
    return aa.length - bb.length;
  });

  const canonicalQuery = pairs.map((p) => `${p.encName}=${p.encValue}`).join("&");
  const canonicalString = [
    FINGERPRINT_VERSION_LABEL,
    method,
    path,
    canonicalQuery,
    "body:empty",
  ].join("\n");

  const fingerprint = createHash("sha256")
    .update(canonicalString, "utf8")
    .digest("hex");

  return { ok: true, fingerprint, canonicalQuery, canonicalString };
}
