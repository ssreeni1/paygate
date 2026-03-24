const CACHE_TTL_MS = 60_000;
export class PricingCacheManager {
    cache = null;
    gatewayUrl;
    constructor(gatewayUrl) {
        this.gatewayUrl = gatewayUrl;
    }
    async getPricing() {
        if (this.cache && Date.now() - this.cache.fetchedAt < CACHE_TTL_MS) {
            return this.cache;
        }
        return this.refresh();
    }
    async refresh() {
        const resp = await fetch(`${this.gatewayUrl}/v1/pricing`);
        if (!resp.ok) {
            throw new Error(`GET /v1/pricing failed: ${resp.status} ${resp.statusText}`);
        }
        const body = await resp.json();
        const endpoints = body.apis.map((api) => ({
            endpoint: api.endpoint,
            method: api.method,
            path: api.path,
            price: api.price,
            priceBaseUnits: api.price_base_units,
            description: api.description,
            dynamic: api.dynamic,
        }));
        this.cache = {
            endpoints,
            recipient: body.recipient,
            token: body.token,
            fetchedAt: Date.now(),
        };
        return this.cache;
    }
    async priceFor(endpoint) {
        const pricing = await this.getPricing();
        return pricing.endpoints.find((e) => e.endpoint === endpoint) ?? null;
    }
    invalidate() {
        this.cache = null;
    }
}
