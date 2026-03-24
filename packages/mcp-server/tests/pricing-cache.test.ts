import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { PricingCacheManager } from '../src/pricing-cache.js';

describe('PricingCacheManager', () => {
  const mockBody = () => JSON.stringify({
    apis: [
      {
        endpoint: 'POST /v1/search',
        method: 'POST',
        path: '/v1/search',
        price: '0.001000',
        price_base_units: 1000,
        description: 'Search the web',
        dynamic: false,
      },
    ],
    recipient: '0xRecipient',
    token: '0xUSDC',
  });

  beforeEach(() => {
    vi.spyOn(globalThis, 'fetch').mockImplementation(async () =>
      new Response(mockBody(), { status: 200 })
    );
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('fetches pricing on first call', async () => {
    const cache = new PricingCacheManager('https://example.com');
    const pricing = await cache.getPricing();
    expect(pricing.endpoints).toHaveLength(1);
    expect(pricing.endpoints[0].endpoint).toBe('POST /v1/search');
    expect(globalThis.fetch).toHaveBeenCalledTimes(1);
  });

  it('returns cached pricing on second call within TTL', async () => {
    const cache = new PricingCacheManager('https://example.com');
    await cache.getPricing();
    await cache.getPricing();
    expect(globalThis.fetch).toHaveBeenCalledTimes(1);
  });

  it('refreshes after invalidate()', async () => {
    const cache = new PricingCacheManager('https://example.com');
    await cache.getPricing();
    cache.invalidate();
    await cache.getPricing();
    expect(globalThis.fetch).toHaveBeenCalledTimes(2);
  });

  it('looks up specific endpoint pricing', async () => {
    const cache = new PricingCacheManager('https://example.com');
    const ep = await cache.priceFor('POST /v1/search');
    expect(ep).not.toBeNull();
    expect(ep!.priceBaseUnits).toBe(1000);
  });

  it('returns null for unknown endpoint', async () => {
    const cache = new PricingCacheManager('https://example.com');
    const ep = await cache.priceFor('POST /v1/unknown');
    expect(ep).toBeNull();
  });
});
