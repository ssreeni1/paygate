import type { PricingInfo, PaymentRequiredResponse, EndpointPricing } from './types.js';

/**
 * Check if a response is a 402 Payment Required.
 */
export function isPaymentRequired(response: Response): boolean {
  return response.status === 402;
}

/**
 * Parse a 402 response into structured pricing info.
 * The JSON body is authoritative per spec.
 */
export async function parse402Response(response: Response): Promise<PaymentRequiredResponse> {
  if (response.status !== 402) {
    throw new Error(`Expected 402 response, got ${response.status}`);
  }

  const body = await response.json() as PaymentRequiredResponse;

  if (!body.pricing) {
    throw new Error('402 response missing pricing field');
  }
  if (!body.pricing.recipient) {
    throw new Error('402 response missing pricing.recipient');
  }
  if (body.pricing.amount_base_units == null) {
    throw new Error('402 response missing pricing.amount_base_units');
  }
  if (!body.pricing.token) {
    throw new Error('402 response missing pricing.token');
  }

  return body;
}

/**
 * Discover pricing for all endpoints by calling the API root.
 * Expects a 402 response with pricing info.
 */
export async function getPricing(baseUrl: string): Promise<Record<string, PricingInfo>> {
  const response = await fetch(baseUrl, { method: 'GET' });

  if (response.status !== 402) {
    throw new Error(`Expected 402 from ${baseUrl}, got ${response.status}`);
  }

  const body = await response.json() as PaymentRequiredResponse;
  const url = new URL(baseUrl);

  return {
    [`GET ${url.pathname}`]: body.pricing,
  };
}

/**
 * Fetch the full pricing map from the gateway's /v1/pricing endpoint.
 * Returns a map of "METHOD /path" -> EndpointPricing.
 */
export async function fetchEndpointPricing(
  baseUrl: string,
): Promise<Map<string, EndpointPricing>> {
  const url = `${baseUrl.replace(/\/$/, '')}/v1/pricing`;
  const response = await fetch(url, { method: 'GET' });

  if (!response.ok) {
    throw new Error(`Failed to fetch pricing from ${url}: ${response.status}`);
  }

  const body = await response.json() as {
    apis: Array<{
      endpoint: string;
      price: string;
      price_base_units: number;
      decimals: number;
      dynamic: boolean;
    }>;
  };

  const map = new Map<string, EndpointPricing>();
  for (const api of body.apis) {
    map.set(api.endpoint, {
      price: api.price,
      priceBaseUnits: api.price_base_units,
      decimals: api.decimals,
      dynamic: api.dynamic ?? false,
    });
  }
  return map;
}
