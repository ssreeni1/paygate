import { PricingCacheManager } from '../pricing-cache.js';
import { SpendTracker } from '../spend-tracker.js';
import type { EstimateInput } from '../types.js';
export declare function handleEstimate(pricingCache: PricingCacheManager, spendTracker: SpendTracker): (input: EstimateInput) => Promise<{
    content: Array<{
        type: "text";
        text: string;
    }>;
    isError?: boolean;
}>;
