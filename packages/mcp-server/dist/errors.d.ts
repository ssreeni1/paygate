import type { PaygateErrorCode, PaygateToolError } from './types.js';
export declare function makeError(code: PaygateErrorCode, message: string, recoverable: boolean): PaygateToolError;
export declare function insufficientBalance(detail: string): PaygateToolError;
export declare function sessionCreationFailed(detail: string): PaygateToolError;
export declare function spendLimitExceeded(spent: string, limit: string, period: 'daily' | 'monthly'): PaygateToolError;
export declare function gatewayUnreachable(detail: string): PaygateToolError;
export declare function invalidInput(detail: string): PaygateToolError;
export declare function upstreamError(status: number, detail: string): PaygateToolError;
export declare function classifyError(err: unknown): PaygateToolError;
export declare function errorToMcpContent(err: PaygateToolError): {
    content: Array<{
        type: 'text';
        text: string;
    }>;
    isError: true;
};
