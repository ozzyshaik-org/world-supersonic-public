import type {
  PaymentPayload,
  PaymentRequirements,
  SettleResponse,
  TransferRecord,
  TransferStatus,
} from "../types.js";

/**
 * Gateway client interface. Production adapter wraps Circle BatchFacilitatorClient
 * + transfer lookup; unit tests inject FakeGatewayClient (obviously synthetic).
 */
export interface GatewayClient {
  readonly name: string;

  settle(
    payload: PaymentPayload,
    requirements: PaymentRequirements,
  ): Promise<SettleResponse>;

  getTransferById(id: string): Promise<TransferRecord | null>;

  /**
   * Search transfers. Network MUST be passed explicitly (search defaults to the
   * client's own network per Circle docs).
   */
  searchTransfers(params: {
    nonce?: string;
    from?: string;
    to?: string;
    network: string;
    status?: TransferStatus;
  }): Promise<TransferRecord[]>;
}
