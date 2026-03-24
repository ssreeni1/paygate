import { describe, it, expect, vi, beforeEach } from 'vitest';
import { handleDiscover } from '../src/tools/discover.js';
import { PricingCacheManager } from '../src/pricing-cache.js';

describe('paygate_discover', () => {
  let pricingCache: PricingCacheManager;

  beforeEach(() => {
    pricingCache = new PricingCacheManager('https://example.com');
    vi.spyOn(pricingCache, 'getPricing').mockResolvedValue({
      endpoints: [
        {
          endpoint: 'POST /v1/search',
          method: 'POST',
          path: '/v1/search',
          price: '0.001000',
          priceBaseUnits: 1000,
          description: 'Search the web for information',
          dynamic: false,
        },
        {
          endpoint: 'POST /v1/image',
          method: 'POST',
          path: '/v1/image',
          price: '0.010000',
          priceBaseUnits: 10000,
          description: 'Generate an image from a prompt',
          dynamic: true,
        },
      ],
      recipient: '0x1234',
      token: '0xUSDC',
      fetchedAt: Date.now(),
    });
  });

  it('returns all APIs without goal', async () => {
    const handler = handleDiscover(pricingCache);
    const result = await handler({});
    const parsed = JSON.parse(result.content[0].text);
    expect(parsed.apis).toHaveLength(2);
    expect(parsed.apis[0].endpoint).toBe('POST /v1/search');
  });

  it('ranks APIs by relevance when goal is provided', async () => {
    const handler = handleDiscover(pricingCache);
    const result = await handler({ goal: 'search for news' });
    const parsed = JSON.parse(result.content[0].text);
    expect(parsed.apis[0].endpoint).toBe('POST /v1/search');
    expect(parsed.goal).toBe('search for news');
  });

  it('returns error when gateway unreachable', async () => {
    vi.spyOn(pricingCache, 'getPricing').mockRejectedValue(new Error('fetch failed'));
    const handler = handleDiscover(pricingCache);
    const result = await handler({});
    expect(result.isError).toBe(true);
    const parsed = JSON.parse(result.content[0].text);
    expect(parsed.error).toBe('gateway_unreachable');
  });
});
