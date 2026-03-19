export interface PricingInfo {
  amount: string;
  amount_base_units: number;
  decimals: number;
  token: string;
  recipient: string;
  quote_id: string;
  quote_expires_at: string;
  methods: string[];
}

export interface PaymentRequiredResponse {
  error: string;
  message: string;
  help_url: string;
  pricing: PricingInfo;
}

export interface PaymentHeaders {
  'X-Payment-Tx': string;
  'X-Payment-Payer': string;
  'X-Payment-Quote-Id'?: string;
}

export interface ReceiptInfo {
  tx_hash: string;
  payer_address: string;
  amount: number;
  verified_at: number;
  status: string;
}

export interface PayGateClientOptions {
  /** Function that executes on-chain payment. Returns tx hash. */
  payFunction: (params: PaymentParams) => Promise<string>;
  /** Payer's wallet address */
  payerAddress: string;
  /** Max retries after payment (default: 1) */
  maxRetries?: number;
}

export interface PaymentParams {
  to: string;
  amount: bigint;
  token: string;
  memo: string;
}
