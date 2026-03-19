import type { PricingInfo, PaymentRequiredResponse } from './types.js';

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
