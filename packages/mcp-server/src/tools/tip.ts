import type { PayGateClient } from '@paygate/sdk';
import { randomBytes } from 'node:crypto';
import { SpendTracker, formatUsd } from '../spend-tracker.js';
import { classifyError, errorToMcpContent, spendLimitExceeded, invalidInput } from '../errors.js';
import type { McpServerConfig, TipInput, TipConfirmInput, SessionTipRecord } from '../types.js';

// ── Pending confirmation store ──

interface PendingTip {
  input: TipInput;
  createdAt: number;
}

const pendingConfirmations = new Map<string, PendingTip>();

const CONFIRMATION_TTL_MS = 60_000; // 60 seconds

function pruneExpired(): void {
  const now = Date.now();
  for (const [token, pending] of pendingConfirmations) {
    if (now - pending.createdAt > CONFIRMATION_TTL_MS) {
      pendingConfirmations.delete(token);
    }
  }
}

// ── Shared tip execution ──

async function executeTip(
  client: PayGateClient,
  config: McpServerConfig,
  spendTracker: SpendTracker,
  sessionTips: SessionTipRecord[],
  input: TipInput,
): Promise<{ content: Array<{ type: 'text'; text: string }>; isError?: boolean }> {
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

  // Record spend based on ACTUAL cost from headers, not the requested amount
  const amountBaseUnits = Math.round(input.amount * 1_000_000);
  const actualCostBaseUnits = costHeader
    ? Math.round(parseFloat(costHeader) * 1_000_000)
    : amountBaseUnits;

  spendTracker.record_spend(actualCostBaseUnits);

  const explorerLink = txHash
    ? `https://testnet.tempo.xyz/tx/${txHash}`
    : null;

  sessionTips.push({
    target: input.target,
    recipient: (parsed.recipient as string) ?? input.target,
    resolvedGithub: (parsed.resolved_github as string) ?? null,
    amount: input.amount,
    amountBaseUnits: actualCostBaseUnits,
    status: (parsed.status as string) ?? 'confirmed',
    receiptUrl: (parsed.receipt_url as string) ?? null,
    txHash: txHash ?? (parsed.tx_hash as string) ?? null,
    tipId: (parsed.tip_id as string) ?? null,
    timestamp: Date.now(),
  });

  process.stderr.write(
    `[paygate] tip ${input.target} — ${formatUsd(actualCostBaseUnits)}` +
    (explorerLink ? ` — ${explorerLink}` : '') + '\n'
  );

  return {
    content: [{
      type: 'text',
      text: JSON.stringify({
        ...parsed,
        payment: {
          cost: formatUsd(actualCostBaseUnits),
          explorerLink: explorerLink ?? 'N/A',
        },
      }, null, 2),
    }],
  };
}

// ── tip_open_source handler ──

export function handleTip(
  client: PayGateClient,
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

    // Confirmation gate for tips >= $1.00
    if (input.amount >= 1.0) {
      pruneExpired();
      const token = randomBytes(16).toString('hex');
      pendingConfirmations.set(token, { input, createdAt: Date.now() });
      return {
        content: [{
          type: 'text',
          text: JSON.stringify({
            confirmation_required: true,
            amount: `$${input.amount.toFixed(2)}`,
            target: input.target,
            reason: input.reason,
            token,
            message: `This tip is $${input.amount.toFixed(2)}. To confirm, call tip_confirm with token: ${token}`,
            expires_in_seconds: 60,
          }, null, 2),
        }],
      };
    }

    try {
      return await executeTip(client, config, spendTracker, sessionTips, input);
    } catch (err) {
      return errorToMcpContent(classifyError(err));
    }
  };
}

// ── tip_confirm handler ──

export function handleTipConfirm(
  client: PayGateClient,
  config: McpServerConfig,
  spendTracker: SpendTracker,
  sessionTips: SessionTipRecord[],
) {
  return async (input: TipConfirmInput): Promise<{
    content: Array<{ type: 'text'; text: string }>;
    isError?: boolean;
  }> => {
    if (!input.token) {
      return errorToMcpContent(invalidInput('token is required'));
    }

    pruneExpired();

    const pending = pendingConfirmations.get(input.token);
    if (!pending) {
      return errorToMcpContent(invalidInput('Invalid or expired confirmation token'));
    }

    pendingConfirmations.delete(input.token);

    const limitViolation = spendTracker.checkLimit(
      Math.round(pending.input.amount * 1_000_000)
    );
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
      return await executeTip(client, config, spendTracker, sessionTips, pending.input);
    } catch (err) {
      return errorToMcpContent(classifyError(err));
    }
  };
}
