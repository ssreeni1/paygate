import { PricingCacheManager } from '../pricing-cache.js';
import type { DiscoverInput, EndpointPricing } from '../types.js';
import { classifyError, errorToMcpContent } from '../errors.js';
import { formatUsd } from '../spend-tracker.js';

export function handleDiscover(pricingCache: PricingCacheManager) {
  return async (input: DiscoverInput): Promise<{
    content: Array<{ type: 'text'; text: string }>;
    isError?: boolean;
  }> => {
    try {
      const pricing = await pricingCache.getPricing();
      let endpoints = pricing.endpoints;

      if (input.goal) {
        endpoints = rankByGoal(endpoints, input.goal);
      }

      const result = endpoints.map((ep) => ({
        endpoint: ep.endpoint,
        description: ep.description,
        price: formatUsd(ep.priceBaseUnits),
        dynamic: ep.dynamic,
        ...(input.goal ? { relevance: computeRelevanceNote(ep, input.goal) } : {}),
      }));

      return {
        content: [{
          type: 'text',
          text: JSON.stringify({
            apis: result,
            gateway: (pricingCache as any).gatewayUrl,
            ...(input.goal ? { goal: input.goal, note: 'APIs ranked by estimated relevance to your goal' } : {}),
          }, null, 2),
        }],
      };
    } catch (err) {
      return errorToMcpContent(classifyError(err));
    }
  };
}

function rankByGoal(endpoints: EndpointPricing[], goal: string): EndpointPricing[] {
  const goalTokens = tokenize(goal);
  const scored = endpoints.map((ep) => {
    const epTokens = tokenize(ep.description + ' ' + ep.path + ' ' + ep.endpoint);
    const overlap = goalTokens.filter((t) => epTokens.includes(t)).length;
    return { ep, score: overlap };
  });
  scored.sort((a, b) => b.score - a.score);
  return scored.map((s) => s.ep);
}

function tokenize(text: string): string[] {
  return text.toLowerCase().replace(/[^a-z0-9\s]/g, ' ').split(/\s+/).filter(Boolean);
}

function computeRelevanceNote(ep: EndpointPricing, goal: string): string {
  const goalTokens = tokenize(goal);
  const epTokens = tokenize(ep.description + ' ' + ep.path);
  const matches = goalTokens.filter((t) => epTokens.includes(t));
  if (matches.length === 0) return 'No direct keyword match — may still be useful';
  return `Matches: ${matches.join(', ')}`;
}
