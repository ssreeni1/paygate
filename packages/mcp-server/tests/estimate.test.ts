import { describe, it, expect, vi, beforeEach } from 'vitest';
import { handleEstimate } from '../src/tools/estimate.js';
import { PricingCacheManager } from '../src/pricing-cache.js';
import { SpendTracker } from '../src/spend-tracker.js';

describe('paygate_estimate', () => {
  let pricingCache: PricingCacheManager;
  let spendTracker: SpendTracker;

  beforeEach(() => {
    pricingCache = new PricingCacheManager('https://example.com');
    spendTracker = new SpendTracker(5_000_000, null);
    vi.spyOn(pricingCache, 'getPricing').mockResolvedValue({
      endpoints: [
        {
          endpoint: 'POST /v1/search',
          method: 'POST',
          path: '/v1/search',
          price: '0.001000',
          priceBaseUnits: 1000,
          description: 'Search',
          dynamic: false,
        },
        {
          endpoint: 'POST /v1/summarize',
          method: 'POST',
          path: '/v1/summarize',
          price: '0.005000',
          priceBaseUnits: 5000,
          description: 'Summarize',
          dynamic: true,
        },
      ],
      recipient: '0x1234',
      token: '0xUSDC',
      fetchedAt: Date.now(),
    });
  });

  it('computes total cost correctly', async () => {
    const handler = handleEstimate(pricingCache, spendTracker);
    const result = await handler({
      calls: [
        { endpoint: 'POST /v1/search', count: 3 },
        { endpoint: 'POST /v1/summarize', count: 1 },
      ],
    });
    const parsed = JSON.parse(result.content[0].text);
    expect(parsed.estimatedTotal).toBe('$0.008000');
    expect(parsed.withinBudget).toBe(true);
    expect(parsed.breakdown).toHaveLength(2);
  });

  it('marks dynamic endpoints as approximate', async () => {
    const handler = handleEstimate(pricingCache, spendTracker);
    const result = await handler({
      calls: [{ endpoint: 'POST /v1/summarize', count: 1 }],
    });
    const parsed = JSON.parse(result.content[0].text);
    expect(parsed.breakdown[0].approximate).toBe(true);
    expect(parsed.note).toBeDefined();
  });

  it('reports withinBudget=false when over limit', async () => {
    spendTracker.record_spend(4_999_000);
    const handler = handleEstimate(pricingCache, spendTracker);
    const result = await handler({
      calls: [{ endpoint: 'POST /v1/search', count: 5 }],
    });
    const parsed = JSON.parse(result.content[0].text);
    expect(parsed.withinBudget).toBe(false);
  });

  it('handles unknown endpoint gracefully', async () => {
    const handler = handleEstimate(pricingCache, spendTracker);
    const result = await handler({
      calls: [{ endpoint: 'POST /v1/unknown', count: 1 }],
    });
    const parsed = JSON.parse(result.content[0].text);
    expect(parsed.breakdown[0].price).toBe('unknown');
  });

  it('returns error for empty calls array', async () => {
    const handler = handleEstimate(pricingCache, spendTracker);
    const result = await handler({ calls: [] });
    expect(result.isError).toBe(true);
  });
});
