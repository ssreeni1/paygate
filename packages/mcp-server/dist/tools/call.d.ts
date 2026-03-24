import type { PayGateClient } from '@paygate/sdk';
import { SpendTracker } from '../spend-tracker.js';
import { SessionManager } from '../session-manager.js';
import { PricingCacheManager } from '../pricing-cache.js';
import type { CallInput, McpServerConfig, ActiveTrace } from '../types.js';
export declare function handleCall(client: PayGateClient, config: McpServerConfig, spendTracker: SpendTracker, sessionManager: SessionManager, pricingCache: PricingCacheManager, activeTraces: Map<string, ActiveTrace>): (input: CallInput) => Promise<{
    content: Array<{
        type: "text";
        text: string;
    }>;
    isError?: boolean;
}>;
