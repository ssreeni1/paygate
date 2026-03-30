import { SpendTracker, formatUsd } from '../spend-tracker.js';
import { classifyError, errorToMcpContent, spendLimitExceeded, invalidInput } from '../errors.js';
import type { McpServerConfig, TipInput, SessionTipRecord } from '../types.js';

export function handleTip(
  config: McpServerConfig,
  spendTracker: SpendTracker,
  sessionTips: SessionTipRecord[],
) {
  return async (input: TipInput): Promise<{
    content: Array<{ type: 'text'; text: string }>;
    isError?: boolean;
  }> => {
    if (!input.target) {
      return errorToMcpContent(invalidInput('target is required (npm package name or @github_username)'));
    }
    if (!input.amount || input.amount <= 0) {
      return errorToMcpContent(invalidInput('amount must be a positive number (USD)'));
    }
    if (!input.reason) {
      return errorToMcpContent(invalidInput('reason is required'));
    }

    const amountBaseUnits = Math.round(input.amount * 1_000_000);

    const limitViolation = spendTracker.checkLimit(amountBaseUnits);
    if (limitViolation) {
      return errorToMcpContent(
        spendLimitExceeded(
          formatUsd(limitViolation.spent),
          formatUsd(limitViolation.limit),
          limitViolation.period
        )
      );
    }

    try {
      const url = `${config.gatewayUrl}/paygate/tip`;
      const body: Record<string, unknown> = {
        target: input.target,
        amount_usd: input.amount,
        reason: input.reason,
        sender_name: config.agentName,
      };
      if (input.evidence) {
        body.evidence = input.evidence;
      }

      const response = await fetch(url, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
      });

      const responseBody = await response.text();
      let parsed: Record<string, unknown>;
      try {
        parsed = JSON.parse(responseBody) as Record<string, unknown>;
      } catch {
        return {
          content: [{ type: 'text', text: `Gateway returned non-JSON: ${responseBody.slice(0, 200)}` }],
          isError: true,
        };
      }

      if (response.status >= 400) {
        return {
          content: [{
            type: 'text',
            text: JSON.stringify({ status: response.status, body: parsed }, null, 2),
          }],
          isError: true,
        };
      }

      spendTracker.record_spend(amountBaseUnits);

      sessionTips.push({
        target: input.target,
        recipient: (parsed.recipient as string) ?? input.target,
        resolvedGithub: (parsed.resolved_github as string) ?? null,
        amount: input.amount,
        amountBaseUnits,
        status: (parsed.status as string) ?? 'confirmed',
        receiptUrl: (parsed.receipt_url as string) ?? null,
        txHash: (parsed.tx_hash as string) ?? null,
        tipId: (parsed.tip_id as string) ?? null,
        timestamp: Date.now(),
      });

      process.stderr.write(
        `[paygate] tip ${input.target} — ${formatUsd(amountBaseUnits)}` +
        (parsed.receipt_url ? ` — ${parsed.receipt_url}` : '') + '\n'
      );

      const result: Record<string, unknown> = {
        ...parsed,
        payment: {
          cost: formatUsd(amountBaseUnits),
        },
      };

      if (input.amount >= 1.0) {
        result._confirmation_note =
          'This tip is >= $1.00. Please confirm with the human operator before proceeding with additional high-value tips.';
      }

      return {
        content: [{
          type: 'text',
          text: JSON.stringify(result, null, 2),
        }],
      };
    } catch (err) {
      return errorToMcpContent(classifyError(err));
    }
  };
}
