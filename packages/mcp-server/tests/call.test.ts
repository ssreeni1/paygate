import { describe, it, expect, vi, beforeEach } from 'vitest';
import { handleCall } from '../src/tools/call.js';
import { SpendTracker } from '../src/spend-tracker.js';
import { SessionManager } from '../src/session-manager.js';
import { PricingCacheManager } from '../src/pricing-cache.js';
import type { McpServerConfig, ActiveTrace } from '../src/types.js';

describe('paygate_call', () => {
  const mockConfig: McpServerConfig = {
    gatewayUrl: 'https://example.com',
    privateKey: '0x' + 'ab'.repeat(32),
    payerAddress: '0x1234',
    agentName: 'test',
    sessionDeposit: '0.10',
    spendLimitDaily: 5_000_000,
    spendLimitMonthly: null,
  };

  it('rejects when spend limit would be exceeded', async () => {
    const spendTracker = new SpendTracker(5_000_000, null);
    spendTracker.record_spend(4_999_500);

    const pricingCache = new PricingCacheManager('https://example.com');
    vi.spyOn(pricingCache, 'priceFor').mockResolvedValue({
      endpoint: 'POST /v1/search',
      method: 'POST',
      path: '/v1/search',
      price: '0.001000',
      priceBaseUnits: 1000,
      description: 'Search',
      dynamic: false,
    });

    const mockClient = {} as any;
    const sessionManager = new SessionManager(mockClient, mockConfig);
    const traces = new Map<string, ActiveTrace>();

    const handler = handleCall(mockClient, mockConfig, spendTracker, sessionManager, pricingCache, traces);
    const result = await handler({ method: 'POST', path: '/v1/search', body: { query: 'test' } });

    expect(result.isError).toBe(true);
    const parsed = JSON.parse(result.content[0].text);
    expect(parsed.error).toBe('spend_limit_exceeded');
  });

  it('rejects invalid method', async () => {
    const spendTracker = new SpendTracker(null, null);
    const pricingCache = new PricingCacheManager('https://example.com');
    const mockClient = {} as any;
    const sessionManager = new SessionManager(mockClient, mockConfig);
    const traces = new Map<string, ActiveTrace>();

    const handler = handleCall(mockClient, mockConfig, spendTracker, sessionManager, pricingCache, traces);
    const result = await handler({ method: 'PATCH', path: '/v1/search' });

    expect(result.isError).toBe(true);
    const parsed = JSON.parse(result.content[0].text);
    expect(parsed.error).toBe('invalid_input');
  });

  it('rejects missing method/path', async () => {
    const spendTracker = new SpendTracker(null, null);
    const pricingCache = new PricingCacheManager('https://example.com');
    const mockClient = {} as any;
    const sessionManager = new SessionManager(mockClient, mockConfig);
    const traces = new Map<string, ActiveTrace>();

    const handler = handleCall(mockClient, mockConfig, spendTracker, sessionManager, pricingCache, traces);
    const result = await handler({ method: '', path: '' });

    expect(result.isError).toBe(true);
  });
});
