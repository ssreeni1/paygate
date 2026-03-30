import { formatUsd } from '../spend-tracker.js';
import type { SessionTipRecord } from '../types.js';

export function handleTipReport(
  sessionTips: SessionTipRecord[],
) {
  return async (): Promise<{
    content: Array<{ type: 'text'; text: string }>;
  }> => {
    if (sessionTips.length === 0) {
      return {
        content: [{
          type: 'text',
          text: 'No tips have been made in this session.',
        }],
      };
    }

    const lines: string[] = [];
    lines.push('## Tip Report');
    lines.push('');
    lines.push('| Package | Recipient | Amount | Status | Receipt URL |');
    lines.push('|---------|-----------|--------|--------|-------------|');

    let totalBaseUnits = 0;
    for (const tip of sessionTips) {
      const pkg = tip.target;
      const recipient = tip.resolvedGithub ?? tip.recipient;
      const amount = formatUsd(tip.amountBaseUnits);
      const status = tip.status;
      const receipt = tip.receiptUrl ?? 'N/A';
      lines.push(`| ${pkg} | ${recipient} | ${amount} | ${status} | ${receipt} |`);
      totalBaseUnits += tip.amountBaseUnits;
    }

    lines.push('');
    lines.push(`**Total: ${formatUsd(totalBaseUnits)}** across ${sessionTips.length} tip(s)`);

    return {
      content: [{
        type: 'text',
        text: lines.join('\n'),
      }],
    };
  };
}
