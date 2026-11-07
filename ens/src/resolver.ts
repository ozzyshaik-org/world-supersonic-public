import { createPublicClient, http, type PublicClient, type Transport } from "viem";
import { sepolia } from "viem/chains";
import { normalize } from "viem/ens";

/** Public Sepolia RPC used when the caller does not supply a transport. Read-only (eth_call only). */
export const DEFAULT_SEPOLIA_RPC = "https://ethereum-sepolia-rpc.publicnode.com";

/** Text record keys must be short printable ASCII without whitespace (ENSIP-5 style keys such as "url", "com.github"). */
const TEXT_KEY = /^[\x21-\x7e]{1,64}$/;
export const MAX_TEXT_KEYS = 16;

export class EnsInputError extends Error {
  readonly code: "MALFORMED_NAME" | "BAD_TEXT_KEY";
  constructor(code: "MALFORMED_NAME" | "BAD_TEXT_KEY", message: string) {
    super(message);
    this.name = "EnsInputError";
    this.code = code;
  }
}

export interface EnsResolution {
  /** The name exactly as supplied. */
  inputName: string;
  /** ENSIP-15 normalized name that was actually resolved. */
  name: string;
  /** Checksummed address from the addr record, or null when the name has none. */
  address: `0x${string}` | null;
  /** Requested text keys; null where the record is unset. */
  texts: Record<string, string | null>;
  chainId: number;
}

/** ENSIP-15 normalisation. Throws EnsInputError for anything that is not a valid multi-label name. */
export function normalizeName(input: string): string {
  let n: string;
  try {
    n = normalize(input);
  } catch (e) {
    throw new EnsInputError("MALFORMED_NAME", `invalid ENS name: ${(e as Error).message.split("\n")[0]}`);
  }
  const labels = n.split(".");
  if (labels.length < 2 || labels.some((l) => l.length === 0)) {
    throw new EnsInputError("MALFORMED_NAME", "invalid ENS name: needs at least two non-empty labels");
  }
  return n;
}

export function validateTextKeys(keys: readonly string[]): string[] {
  if (keys.length > MAX_TEXT_KEYS) throw new EnsInputError("BAD_TEXT_KEY", `at most ${MAX_TEXT_KEYS} text keys`);
  for (const k of keys) {
    if (!TEXT_KEY.test(k)) throw new EnsInputError("BAD_TEXT_KEY", `invalid text key: ${JSON.stringify(k)}`);
  }
  return [...new Set(keys)];
}

export function sepoliaClient(transport?: Transport): PublicClient {
  return createPublicClient({ chain: sepolia, transport: transport ?? http(DEFAULT_SEPOLIA_RPC) }) as PublicClient;
}

/**
 * Resolve an ENS name's address and the named text records through the ENS Universal Resolver
 * (the upgradeable proxy that viem ships for the client's chain; on Sepolia it walks the ENSv2 registries).
 * Read-only: issues eth_call only. Never fetches any URL found in a record.
 */
export async function resolveEns(
  inputName: string,
  textKeys: readonly string[] = [],
  client: PublicClient = sepoliaClient(),
): Promise<EnsResolution> {
  const name = normalizeName(inputName);
  const keys = validateTextKeys(textKeys);
  const [address, ...values] = await Promise.all([
    client.getEnsAddress({ name }),
    ...keys.map((key) => client.getEnsText({ name, key })),
  ]);
  const texts: Record<string, string | null> = {};
  keys.forEach((k, i) => {
    const v = values[i];
    texts[k] = v ?? null; // viem already maps an empty text record to null (checked by planted-bug run)
  });
  return { inputName, name, address: address ?? null, texts, chainId: client.chain?.id ?? 0 };
}
