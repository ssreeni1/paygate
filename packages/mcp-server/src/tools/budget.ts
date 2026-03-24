import { SpendTracker, formatUsd } from '../spend-tracker.js';
import { SessionManager } from '../session-manager.js';
import type { McpServerConfig } from '../types.js';

export function handleBudget(
  spendTracker: SpendTracker,
  sessionManager: SessionManager,
  config: McpServerConfig,
) {
  return async (): Promise<{
    content: Array<{ type: 'text'; text: string }>;
  }> => {
    const record = spendTracker.getRecord();
    const session = sessionManager.getSession();

    const result = {
      session: session
        ? {
            sessionId: session.sessionId,
            balance: formatUsd(session.balance),
            expiresAt: session.expiresAt,
          }
        : null,
      spending: {
        totalSpentToday: formatUsd(record.totalSpentToday),
        totalSpentThisMonth: formatUsd(record.totalSpentThisMonth),
        callCount: record.callCount,
      },
      limits: {
        daily: config.spendLimitDaily !== null ? formatUsd(config.spendLimitDaily) : 'unlimited',
        monthly: config.spendLimitMonthly !== null ? formatUsd(config.spendLimitMonthly) : 'unlimited',
        remainingDaily: spendTracker.remainingDaily() === Infinity
          ? 'unlimited'
          : formatUsd(spendTracker.remainingDaily()),
        remainingMonthly: spendTracker.remainingMonthly() === Infinity
          ? 'unlimited'
          : formatUsd(spendTracker.remainingMonthly()),
      },
      agent: config.agentName,
    };

    return {
      content: [{
        type: 'text',
        text: JSON.stringify(result, null, 2),
      }],
    };
  };
}
