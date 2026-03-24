import { describe, it, expect } from 'vitest';
import { handleBudget } from '../src/tools/budget.js';
import { SpendTracker } from '../src/spend-tracker.js';
import { SessionManager } from '../src/session-manager.js';
import type { McpServerConfig } from '../src/types.js';

describe('paygate_budget', () => {
  const config: McpServerConfig = {
    gatewayUrl: 'https://example.com',
    privateKey: '0x' + 'ab'.repeat(32),
    payerAddress: '0x1234',
    agentName: 'test-agent',
    sessionDeposit: '0.10',
    spendLimitDaily: 5_000_000,
    spendLimitMonthly: 50_000_000,
  };

  it('returns complete budget info with session', async () => {
    const spendTracker = new SpendTracker(5_000_000, 50_000_000);
    spendTracker.record_spend(1_000_000);

    const mockClient = {} as any;
    const sessionManager = new SessionManager(mockClient, config);
    sessionManager.setSession({
      sessionId: 'sess_abc',
      sessionSecret: 'ssec_def',
      balance: 90_000,
      ratePerRequest: 1_000,
      expiresAt: new Date(Date.now() + 3600_000).toISOString(),
      gatewayBaseUrl: 'https://example.com',
    });

    const handler = handleBudget(spendTracker, sessionManager, config);
    const result = await handler();
    const parsed = JSON.parse(result.content[0].text);

    expect(parsed.session.sessionId).toBe('sess_abc');
    expect(parsed.session.balance).toBe('$0.090000');
    expect(parsed.spending.totalSpentToday).toBe('$1.000000');
    expect(parsed.limits.daily).toBe('$5.000000');
    expect(parsed.limits.remainingDaily).toBe('$4.000000');
    expect(parsed.agent).toBe('test-agent');
  });

  it('returns null session when none active', async () => {
    const spendTracker = new SpendTracker(null, null);
    const mockClient = {} as any;
    const sessionManager = new SessionManager(mockClient, config);

    const handler = handleBudget(spendTracker, sessionManager, config);
    const result = await handler();
    const parsed = JSON.parse(result.content[0].text);

    expect(parsed.session).toBeNull();
    expect(parsed.limits.daily).toBe('$5.000000');
  });
});
