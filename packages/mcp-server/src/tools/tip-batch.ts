import type { PayGateClient } from '@paygate/sdk';
import { SpendTracker, formatUsd } from '../spend-tracker.js';
import { classifyError, errorToMcpContent, spendLimitExceeded, invalidInput } from '../errors.js';
import type { McpServerConfig, TipBatchInput, SessionTipRecord } from '../types.js';

export function handleTipBatch(
  client: PayGateClient,
  config: McpServerConfig,
  spendTracker: SpendTracker,
  sessionTips: SessionTipRecord[],
) {
  return async (input: TipBatchInput): Promise<{
    content: Array<{ type: 'text'; text: string }>;
    isError?: boolean;
  }> => {
    if (!input.tips || !Array.isArray(input.tips) || input.tips.length === 0) {
      return errorToMcpContent(invalidInput('tips array is required and must not be empty'));
    }

    for (let i = 0; i < input.tips.length; i++) {
      const tip = input.tips[i];
      if (!tip.target) {
        return errorToMcpContent(invalidInput(`tips[${i}].target is required`));
      }
      if (!tip.amount || tip.amount <= 0) {
        return errorToMcpContent(invalidInput(`tips[${i}].amount must be a positive number (USD)`));
      }
      if (!tip.reason) {
        return errorToMcpContent(invalidInput(`tips[${i}].reason is required`));
      }
    }

    const totalAmount = input.tips.reduce((sum, t) => sum + t.amount, 0);
    const totalBaseUnits = Math.round(totalAmount * 1_000_000);

    const limitViolation = spendTracker.checkLimit(totalBaseUnits);
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
      const url = `${config.gatewayUrl}/paygate/tip/batch`;
      const body: Record<string, unknown> = {
        tips: input.tips.map((t) => ({
          target: t.target,
          amount_usd: t.amount,
          reason: t.reason,
          ...(t.evidence ? { evidence: t.evidence } : {}),
        })),
        sender_name: input.sender_name ?? config.agentName,
      };

      const response = await client.fetch(url, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
      });

      const costHeader = response.headers.get('X-Payment-Cost');
      const txHash = response.headers.get('X-Payment-Tx');

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

      // Record spend based on ACTUAL succeeded amount from response, not total requested
      const summary = parsed.summary as Record<string, unknown> | undefined;
      const succeededAmountUsd = summary?.total_amount_usd as number | undefined;
      const actualCostBaseUnits = costHeader
        ? Math.round(parseFloat(costHeader) * 1_000_000)
        : succeededAmountUsd != null
          ? Math.round(succeededAmountUsd * 1_000_000)
          : totalBaseUnits;

      spendTracker.record_spend(actualCostBaseUnits);

      const explorerLink = txHash
        ? `https://testnet.tempo.xyz/tx/${txHash}`
        : null;

      const results = (parsed.results ?? parsed.tips ?? []) as Array<Record<string, unknown>>;
      for (let i = 0; i < results.length; i++) {
        const r = results[i];
        const tip = input.tips[i];
        sessionTips.push({
          target: tip?.target ?? (r.recipient as string) ?? 'unknown',
          recipient: (r.recipient as string) ?? tip?.target ?? 'unknown',
          resolvedGithub: (r.resolved_github as string) ?? null,
          amount: tip?.amount ?? 0,
          amountBaseUnits: Math.round((tip?.amount ?? 0) * 1_000_000),
          status: (r.status as string) ?? 'confirmed',
          receiptUrl: (r.receipt_url as string) ?? null,
          txHash: txHash ?? (r.tx_hash as string) ?? null,
          tipId: (r.tip_id as string) ?? null,
          timestamp: Date.now(),
        });
      }

      process.stderr.write(
        `[paygate] batch tip — ${results.length} tips — total: ${formatUsd(actualCostBaseUnits)}` +
        (explorerLink ? ` — ${explorerLink}` : '') + '\n'
      );

      return {
        content: [{
          type: 'text',
          text: JSON.stringify({
            tipsCount: results.length,
            totalAmount: formatUsd(actualCostBaseUnits),
            results,
            payment: {
              cost: formatUsd(actualCostBaseUnits),
              explorerLink: explorerLink ?? 'N/A',
            },
          }, null, 2),
        }],
      };
    } catch (err) {
      return errorToMcpContent(classifyError(err));
    }
  };
}
