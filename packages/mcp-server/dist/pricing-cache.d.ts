import type { PricingCache, EndpointPricing } from './types.js';
export declare class PricingCacheManager {
    private cache;
    private gatewayUrl;
    constructor(gatewayUrl: string);
    getPricing(): Promise<PricingCache>;
    refresh(): Promise<PricingCache>;
    priceFor(endpoint: string): Promise<EndpointPricing | null>;
    invalidate(): void;
}
