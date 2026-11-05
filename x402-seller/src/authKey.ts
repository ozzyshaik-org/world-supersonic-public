/**
 * auth_key = <network>|<payer>|<nonce>
 * - network exactly as in accepted payment requirements
 * - payer lowercase 0x hex
 * - nonce lowercase 0x + 64 hex chars (exactly 32 bytes)
 */

const ADDRESS_RE = /^0x[0-9a-fA-F]{40}$/;
const NONCE_RE = /^0x[0-9a-fA-F]{64}$/;

export class AuthKeyError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "AuthKeyError";
  }
}

export function normalizePayer(payer: string): string {
  if (!ADDRESS_RE.test(payer)) {
    throw new AuthKeyError(`invalid payer address: ${payer}`);
  }
  return payer.toLowerCase();
}

export function normalizeNonce(nonce: string): string {
  if (!NONCE_RE.test(nonce)) {
    throw new AuthKeyError(
      `invalid nonce: must be 0x + 64 hex characters (32 bytes), got ${nonce}`,
    );
  }
  return nonce.toLowerCase();
}

export function buildAuthKey(network: string, payer: string, nonce: string): string {
  if (!network || network.includes("|")) {
    throw new AuthKeyError("invalid network for auth_key");
  }
  const p = normalizePayer(payer);
  const n = normalizeNonce(nonce);
  return `${network}|${p}|${n}`;
}

export function parseAuthKey(authKey: string): {
  network: string;
  payer: string;
  nonce: string;
} {
  const parts = authKey.split("|");
  if (parts.length !== 3) {
    throw new AuthKeyError(`auth_key must have exactly 3 segments, got ${parts.length}`);
  }
  const [network, payer, nonce] = parts as [string, string, string];
  return {
    network,
    payer: normalizePayer(payer),
    nonce: normalizeNonce(nonce),
  };
}

export function assertNonce32Bytes(nonce: string): string {
  return normalizeNonce(nonce);
}
