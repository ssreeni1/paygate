import type { PricingCache, EndpointPricing } from './types.js';

const CACHE_TTL_MS = 60_000;

export class PricingCacheManager {
  private cache: PricingCache | null = null;
  private gatewayUrl: string;

  constructor(gatewayUrl: string) {
    this.gatewayUrl = gatewayUrl;
  }

  async getPricing(): Promise<PricingCache> {
    if (this.cache && Date.now() - this.cache.fetchedAt < CACHE_TTL_MS) {
      return this.cache;
    }
    return this.refresh();
  }

  async refresh(): Promise<PricingCache> {
    const resp = await fetch(`${this.gatewayUrl}/v1/pricing`);
    if (!resp.ok) {
      throw new Error(`GET /v1/pricing failed: ${resp.status} ${resp.statusText}`);
    }

    const body = await resp.json() as {
      apis: Array<{
        endpoint: string;
        method: string;
        path: string;
        price: string;
        price_base_units: number;
        description: string;
        dynamic: boolean;
      }>;
      recipient: string;
      token: string;
    };

    const endpoints: EndpointPricing[] = body.apis.map((api) => ({
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

  async priceFor(endpoint: string): Promise<EndpointPricing | null> {
    const pricing = await this.getPricing();
    return pricing.endpoints.find((e) => e.endpoint === endpoint) ?? null;
  }

  invalidate(): void {
    this.cache = null;
  }
}
