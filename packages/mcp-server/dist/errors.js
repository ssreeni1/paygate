export function makeError(code, message, recoverable) {
    return { error: code, message, recoverable };
}
export function insufficientBalance(detail) {
    return makeError('insufficient_balance', `Wallet balance too low: ${detail}`, false);
}
export function sessionCreationFailed(detail) {
    return makeError('session_creation_failed', `Session creation failed: ${detail}`, true);
}
export function spendLimitExceeded(spent, limit, period) {
    return makeError('spend_limit_exceeded', `${period} spend limit exceeded: spent ${spent} of ${limit} limit`, false);
}
export function gatewayUnreachable(detail) {
    return makeError('gateway_unreachable', `Cannot reach gateway: ${detail}`, true);
}
export function invalidInput(detail) {
    return makeError('invalid_input', `Invalid input: ${detail}`, false);
}
export function upstreamError(status, detail) {
    return makeError('upstream_error', `Upstream returned ${status}: ${detail}`, true);
}
export function classifyError(err) {
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
export function errorToMcpContent(err) {
    return {
        content: [{ type: 'text', text: JSON.stringify(err, null, 2) }],
        isError: true,
    };
}
