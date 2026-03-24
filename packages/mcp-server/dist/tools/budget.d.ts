import { SpendTracker } from '../spend-tracker.js';
import { SessionManager } from '../session-manager.js';
import type { McpServerConfig } from '../types.js';
export declare function handleBudget(spendTracker: SpendTracker, sessionManager: SessionManager, config: McpServerConfig): () => Promise<{
    content: Array<{
        type: "text";
        text: string;
    }>;
}>;
