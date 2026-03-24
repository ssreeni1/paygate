import type { PaygateErrorCode, PaygateToolError } from './types.js';

export function makeError(
  code: PaygateErrorCode,
  message: string,
  recoverable: boolean
): PaygateToolError {
  return { error: code, message, recoverable };
}

export function insufficientBalance(detail: string): PaygateToolError {
  return makeError('insufficient_balance', `Wallet balance too low: ${detail}`, false);
}

export function sessionCreationFailed(detail: string): PaygateToolError {
  return makeError('session_creation_failed', `Session creation failed: ${detail}`, true);
}

export function spendLimitExceeded(
  spent: string,
  limit: string,
  period: 'daily' | 'monthly'
): PaygateToolError {
  return makeError(
    'spend_limit_exceeded',
    `${period} spend limit exceeded: spent ${spent} of ${limit} limit`,
    false
  );
}

export function gatewayUnreachable(detail: string): PaygateToolError {
  return makeError('gateway_unreachable', `Cannot reach gateway: ${detail}`, true);
}

export function invalidInput(detail: string): PaygateToolError {
  return makeError('invalid_input', `Invalid input: ${detail}`, false);
}

export function upstreamError(status: number, detail: string): PaygateToolError {
  return makeError('upstream_error', `Upstream returned ${status}: ${detail}`, true);
}

export function classifyError(err: unknown): PaygateToolError {
  if (err instanceof Error) {
    const msg = err.message;
    if (msg.includes('ECONNREFUSED') || msg.includes('ETIMEDOUT') || msg.includes('fetch failed')) {
      return gatewayUnreachable(msg);
    }
    if (msg.includes('insufficient') || msg.includes('balance')) {
      return insufficientBalance(msg);
    }
    if (msg.includes('Session creation failed') || msg.includes('nonce')) {
      return sessionCreationFailed(msg);
    }
    return makeError('upstream_error', msg, true);
  }
  return makeError('upstream_error', String(err), true);
}

export function errorToMcpContent(err: PaygateToolError): { content: Array<{ type: 'text'; text: string }>; isError: true } {
  return {
    content: [{ type: 'text', text: JSON.stringify(err, null, 2) }],
    isError: true,
  };
}
