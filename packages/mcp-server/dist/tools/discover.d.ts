import { PricingCacheManager } from '../pricing-cache.js';
import type { DiscoverInput } from '../types.js';
export declare function handleDiscover(pricingCache: PricingCacheManager): (input: DiscoverInput) => Promise<{
    content: Array<{
        type: "text";
        text: string;
    }>;
    isError?: boolean;
}>;
