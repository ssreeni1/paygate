import type { SessionState, McpServerConfig } from './types.js';
export declare class SessionManager {
    private session;
    private config;
    constructor(client: unknown, config: McpServerConfig);
    getSession(): SessionState | null;
    getBalance(): number;
    deductBalance(amount: number): void;
    updateFromSdkResponse(responseHeaders: Headers): void;
    tryResumeSession(): Promise<boolean>;
    setSession(state: SessionState): void;
    invalidate(): void;
    logShutdownState(): void;
}
