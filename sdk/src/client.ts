import { requestHash, paymentMemo } from './hash.js';
import { parse402Response, isPaymentRequired } from './discovery.js';
import { getPricing } from './discovery.js';
import type { PayGateClientOptions, PaymentParams, PricingInfo } from './types.js';

export class PayGateClient {
  private payFunction: (params: PaymentParams) => Promise<string>;
  private payerAddress: string;
  private maxRetries: number;

  constructor(options: PayGateClientOptions) {
    this.payFunction = options.payFunction;
    this.payerAddress = options.payerAddress;
    this.maxRetries = options.maxRetries ?? 1;
  }

  /**
   * Fetch a PayGate-protected URL. Handles 402 -> pay -> retry automatically.
   */
  async fetch(url: string, init?: RequestInit): Promise<Response> {
    const response = await fetch(url, init);

    if (!isPaymentRequired(response)) {
      return response;
    }

    const parsed = await parse402Response(response);
    const pricing = parsed.pricing;

    const method = (init?.method ?? 'GET').toUpperCase();
    const urlObj = new URL(url);
    const path = urlObj.pathname + urlObj.search;
    const body = init?.body ? String(init.body) : '';

    const reqHash = requestHash(method, path, body);
    const memo = paymentMemo(pricing.quote_id, reqHash);

    const txHash = await this.payFunction({
      to: pricing.recipient,
      amount: BigInt(pricing.amount_base_units),
      token: pricing.token,
      memo,
    });

    const paymentHeaders: Record<string, string> = {
      'X-Payment-Tx': txHash,
      'X-Payment-Payer': this.payerAddress,
      'X-Payment-Quote-Id': pricing.quote_id,
    };

    let lastResponse: Response | undefined;
    for (let attempt = 0; attempt < this.maxRetries; attempt++) {
      const existingHeaders: Record<string, string> = {};
      if (init?.headers) {
        if (init.headers instanceof Headers) {
          init.headers.forEach((v, k) => { existingHeaders[k] = v; });
        } else if (Array.isArray(init.headers)) {
          for (const [k, v] of init.headers) { existingHeaders[k] = v; }
        } else {
          Object.assign(existingHeaders, init.headers);
        }
      }

      const retryInit: RequestInit = {
        ...init,
        headers: { ...existingHeaders, ...paymentHeaders },
      };

      lastResponse = await fetch(url, retryInit);

      if (!isPaymentRequired(lastResponse)) {
        return lastResponse;
      }
    }

    throw new Error(
      `Payment was sent (tx: ${txHash}) but gateway still returned 402 after ${this.maxRetries} retry(s)`
    );
  }

  /**
   * Discover pricing without paying.
   */
  async getPricing(baseUrl: string): Promise<Record<string, PricingInfo>> {
    return getPricing(baseUrl);
  }
}
