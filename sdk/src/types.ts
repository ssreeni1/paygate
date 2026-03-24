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

export type FailureMode = 'open' | 'closed';

export interface PayGateClientOptions {
  /** Function that executes on-chain payment. Returns tx hash. */
  payFunction: (params: PaymentParams) => Promise<string>;
  /** Payer's wallet address */
  payerAddress: string;
  /** Max retries after payment (default: 1) */
  maxRetries?: number;
  /** Enable automatic session management */
  autoSession?: boolean;
  /** Deposit amount per session in USDC (default: "0.10") */
  sessionDeposit?: string;
  /** Behavior when gateway is unreachable. 'closed' (default) throws; 'open' bypasses to upstreamUrl. */
  failureMode?: FailureMode;
  /** Required when failureMode is 'open'. Upstream origin URL for bypass. */
  upstreamUrl?: string;
  /** Agent identity string. Sent as X-Payment-Agent on every outgoing request. */
  agentName?: string;
  /** Spend limit in USDC (decimal string). Used by estimateCost() for withinBudget flag. */
  spendLimit?: string;
}

export interface EndpointPricing {
  price: string;
  priceBaseUnits: number;
  decimals: number;
  dynamic: boolean;
}

export interface EstimateCostEntry {
  endpoint: string;
  price: string;
  count: number;
  subtotal: string;
  dynamic: boolean;
}

export interface EstimateCostResult {
  total: string;
  breakdown: EstimateCostEntry[];
  withinBudget: boolean;
}

export interface PaymentParams {
  to: string;
  amount: bigint;
  token: string;
  memo: string;
}

export interface SessionNonceResponse {
  nonce: string;
  expiresAt: string;
}

export interface SessionCreateResponse {
  sessionId: string;
  sessionSecret: string;
  balance: string;
  ratePerRequest: string;
  expiresAt: string;
}

export interface SessionInfo {
  sessionId: string;
  balance: string;
  ratePerRequest: string;
  requestsMade: number;
  expiresAt: string;
  status: string;
}
