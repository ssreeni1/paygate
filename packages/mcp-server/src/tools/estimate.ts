import { PricingCacheManager } from '../pricing-cache.js';
import { SpendTracker, formatUsd } from '../spend-tracker.js';
import type { EstimateInput } from '../types.js';
import { classifyError, errorToMcpContent, invalidInput } from '../errors.js';

export function handleEstimate(
  pricingCache: PricingCacheManager,
  spendTracker: SpendTracker,
) {
  return async (input: EstimateInput): Promise<{
    content: Array<{ type: 'text'; text: string }>;
    isError?: boolean;
  }> => {
    if (!input.calls || !Array.isArray(input.calls) || input.calls.length === 0) {
      return errorToMcpContent(invalidInput('calls array is required and must be non-empty'));
    }

    try {
      const pricing = await pricingCache.getPricing();
      let totalBaseUnits = 0;
      const breakdown: Array<{
        endpoint: string;
        price: string;
        count: number;
        subtotal: string;
        approximate: boolean;
      }> = [];

      for (const call of input.calls) {
        if (!call.endpoint || typeof call.count !== 'number' || call.count < 1) {
          return errorToMcpContent(invalidInput(
            `Invalid call entry: endpoint="${call.endpoint}", count=${call.count}`
          ));
        }

        const ep = pricing.endpoints.find((e) => e.endpoint === call.endpoint);
        if (!ep) {
          breakdown.push({
            endpoint: call.endpoint,
            price: 'unknown',
            count: call.count,
            subtotal: 'unknown',
            approximate: true,
          });
          continue;
        }

        const subtotal = ep.priceBaseUnits * call.count;
        totalBaseUnits += subtotal;
        breakdown.push({
          endpoint: call.endpoint,
          price: formatUsd(ep.priceBaseUnits),
          count: call.count,
          subtotal: formatUsd(subtotal),
          approximate: ep.dynamic,
        });
      }

      const remainingDaily = spendTracker.remainingDaily();
      const withinBudget = remainingDaily === Infinity || totalBaseUnits <= remainingDaily;

      return {
        content: [{
          type: 'text',
          text: JSON.stringify({
            estimatedTotal: formatUsd(totalBaseUnits),
            breakdown,
            withinBudget,
            note: breakdown.some((b) => b.approximate)
              ? 'Some endpoints have dynamic pricing — actual cost may vary'
              : undefined,
          }, null, 2),
        }],
      };
    } catch (err) {
      return errorToMcpContent(classifyError(err));
    }
  };
}
