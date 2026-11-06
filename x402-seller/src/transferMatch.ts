/**
 * Centralized transfer↔binding matching for reconciliation, fulfillment retry,
 * and periodic observation. Spec 01: adopt a transfer only when nonce, sender,
 * recipient and amount equal the stored binding. Never use hits[0] to disambiguate.
 */

import type { AuthorizationBinding, TransferRecord } from "./types.js";

export type LookupOutcome =
  | { kind: "match"; transfer: TransferRecord }
  | { kind: "mismatch"; transfer: TransferRecord }
  | { kind: "empty" }
  | { kind: "ambiguous"; transfers: TransferRecord[] }
  | { kind: "failed"; error: unknown };

export function transferMatchesBinding(
  transfer: TransferRecord,
  binding: Pick<AuthorizationBinding, "nonce" | "payer" | "payTo" | "price" | "network">,
): boolean {
  return (
    transfer.nonce.toLowerCase() === binding.nonce.toLowerCase() &&
    transfer.fromAddress.toLowerCase() === binding.payer.toLowerCase() &&
    transfer.toAddress.toLowerCase() === binding.payTo.toLowerCase() &&
    transfer.amount === binding.price &&
    transfer.sendingNetwork === binding.network
  );
}

/**
 * Classify a successful search result list (never call with a thrown lookup).
 * Zero hits → empty. Exactly one hit → match or mismatch. Two+ → ambiguous.
 */
export function classifySearchHits(
  hits: TransferRecord[],
  binding: Pick<AuthorizationBinding, "nonce" | "payer" | "payTo" | "price" | "network">,
): LookupOutcome {
  if (hits.length === 0) return { kind: "empty" };
  if (hits.length > 1) return { kind: "ambiguous", transfers: hits };
  const transfer = hits[0]!;
  if (transferMatchesBinding(transfer, binding)) {
    return { kind: "match", transfer };
  }
  return { kind: "mismatch", transfer };
}

/**
 * Classify a getTransferById result. null → empty (not found).
 * Found but mismatched fields → mismatch (never adopt).
 */
export function classifyTransferById(
  transfer: TransferRecord | null,
  binding: Pick<AuthorizationBinding, "nonce" | "payer" | "payTo" | "price" | "network">,
): LookupOutcome {
  if (transfer == null) return { kind: "empty" };
  if (transferMatchesBinding(transfer, binding)) {
    return { kind: "match", transfer };
  }
  return { kind: "mismatch", transfer };
}
