// ── Environment config ──

export interface McpServerConfig {
  gatewayUrl: string;
  privateKey: string;
  payerAddress: string;
  agentName: string;
  sessionDeposit: string;
  spendLimitDaily: number | null;
  spendLimitMonthly: number | null;
}

export interface EndpointPricing {
  endpoint: string;
  method: string;
  path: string;
  price: string;
  priceBaseUnits: number;
  description: string;
  dynamic: boolean;
}

export interface PricingCache {
  endpoints: EndpointPricing[];
  recipient: string;
  token: string;
  fetchedAt: number;
}

export interface SessionState {
  sessionId: string;
  sessionSecret: string;
  balance: number;
  ratePerRequest: number;
  expiresAt: string;
  gatewayBaseUrl: string;
}

export interface SpendRecord {
  totalSpentToday: number;
  totalSpentThisMonth: number;
  dayStartUtc: string;
  monthStartUtc: string;
  callCount: number;
}

export interface TraceEntry {
  endpoint: string;
  method: string;
  cost: number;
  timestamp: number;
  explorerLink: string;
}

export interface ActiveTrace {
  name: string;
  startedAt: number;
  entries: TraceEntry[];
}

export interface PaygateToolSuccess<T = unknown> {
  result: T;
  payment?: {
    cost: string;
    explorerLink: string;
    balanceRemaining: string;
  };
}

export interface PaygateToolError {
  error: PaygateErrorCode;
  message: string;
  recoverable: boolean;
}

export type PaygateErrorCode =
  | 'insufficient_balance'
  | 'session_creation_failed'
  | 'spend_limit_exceeded'
  | 'gateway_unreachable'
  | 'invalid_input'
  | 'upstream_error';

export interface DiscoverInput {
  goal?: string;
}

export interface CallInput {
  method: string;
  path: string;
  body?: Record<string, unknown>;
  headers?: Record<string, string>;
}

export interface BudgetInput {}

export interface EstimateInput {
  calls: { endpoint: string; count: number }[];
}

export interface TraceInput {
  action: 'start' | 'stop';
  name: string;
}

export interface TipInput {
  target: string;
  amount: number;
  reason: string;
  evidence?: string;
}

export interface TipConfirmInput {
  token: string;
}

export interface TipBatchInput {
  tips: Array<{
    target: string;
    amount: number;
    reason: string;
    evidence?: string;
  }>;
  sender_name?: string;
}

export interface SessionTipRecord {
  target: string;
  recipient: string;
  resolvedGithub: string | null;
  amount: number;
  amountBaseUnits: number;
  status: string;
  receiptUrl: string | null;
  txHash: string | null;
  tipId: string | null;
  timestamp: number;
}
