import { requestHash, paymentMemo, sessionMemo, hmacSha256 } from './hash.js';
import { parse402Response, isPaymentRequired, fetchEndpointPricing } from './discovery.js';
import { getPricing } from './discovery.js';
import type {
  PayGateClientOptions,
  PaymentParams,
  PricingInfo,
  FailureMode,
  EndpointPricing,
  EstimateCostEntry,
  EstimateCostResult,
} from './types.js';

function formatUsdc(baseUnits: number, decimals: number): string {
  const divisor = 10 ** decimals;
  return (baseUnits / divisor).toFixed(decimals);
}

export class PayGateClient {
  private payFunction: (params: PaymentParams) => Promise<string>;
  private payerAddress: string;
  private maxRetries: number;
  private autoSession: boolean;
  private sessionDeposit: string;

  // Session state
  private sessionId: string | null = null;
  private sessionSecret: string | null = null;
  private sessionBalance: number = 0;
  private sessionRatePerRequest: number = 0;
  private gatewayBaseUrl: string | null = null;

  // Cached pricing from last 402 (used for session deposit)
  private cachedRecipient: string | null = null;
  private cachedToken: string | null = null;

  // Wave 3: failureMode + agentName + estimateCost
  private failureMode: FailureMode;
  private upstreamUrl: string | null;
  private agentName: string | null;
  private spendLimit: number | null;

  // Pricing cache for estimateCost()
  private pricingCache: Map<string, EndpointPricing> | null = null;
  private pricingCacheExpiry: number = 0;
  private static readonly PRICING_CACHE_TTL_MS = 60_000;

  constructor(options: PayGateClientOptions) {
    this.payFunction = options.payFunction;
    this.payerAddress = options.payerAddress;
    this.maxRetries = options.maxRetries ?? 1;
    this.autoSession = options.autoSession ?? false;
    this.sessionDeposit = options.sessionDeposit ?? '0.10';

    this.failureMode = options.failureMode ?? 'closed';
    this.upstreamUrl = options.upstreamUrl ?? null;
    this.agentName = options.agentName ?? null;
    this.spendLimit = options.spendLimit
      ? Math.round(parseFloat(options.spendLimit) * 1_000_000)
      : null;

    if (this.failureMode === 'open' && !this.upstreamUrl) {
      throw new Error("failureMode 'open' requires upstreamUrl to be set");
    }
  }

  /**
   * Fetch a PayGate-protected URL. Handles 402 -> pay -> retry automatically.
   * When failureMode is 'open', bypasses to upstream on network errors.
   */
  async fetch(url: string, init?: RequestInit): Promise<Response> {
    try {
      return await this._fetchInner(url, init);
    } catch (error) {
      if (this.failureMode === 'open' && this.isNetworkError(error)) {
        return this.bypassToUpstream(url, init);
      }
      throw error;
    }
  }

  /**
   * Estimate the cost of a planned set of API calls.
   * Fetches pricing from GET /v1/pricing (cached for 60s).
   */
  async estimateCost(
    gatewayBaseUrl: string,
    calls: { endpoint: string; count: number }[],
  ): Promise<EstimateCostResult> {
    const pricing = await this.getOrFetchPricing(gatewayBaseUrl);

    let totalBaseUnits = 0;
    const breakdown: EstimateCostEntry[] = [];

    for (const call of calls) {
      const endpointPricing = pricing.get(call.endpoint);
      if (!endpointPricing) {
        throw new Error(
          `Unknown endpoint: "${call.endpoint}". ` +
          `Available endpoints: ${[...pricing.keys()].join(', ')}`
        );
      }

      const subtotalBaseUnits = endpointPricing.priceBaseUnits * call.count;
      totalBaseUnits += subtotalBaseUnits;

      breakdown.push({
        endpoint: call.endpoint,
        price: endpointPricing.price,
        count: call.count,
        subtotal: formatUsdc(subtotalBaseUnits, endpointPricing.decimals),
        dynamic: endpointPricing.dynamic,
      });
    }

    const decimals = breakdown.length > 0
      ? (pricing.get(calls[0].endpoint)?.decimals ?? 6)
      : 6;
    const total = formatUsdc(totalBaseUnits, decimals);

    const withinBudget = this.spendLimit === null
      ? true
      : totalBaseUnits <= this.spendLimit;

    return { total, breakdown, withinBudget };
  }

  /**
   * Discover pricing without paying.
   */
  async getPricing(baseUrl: string): Promise<Record<string, PricingInfo>> {
    return getPricing(baseUrl);
  }

  private async _fetchInner(url: string, init?: RequestInit): Promise<Response> {
    const method = (init?.method ?? 'GET').toUpperCase();
    const urlObj = new URL(url);
    const path = urlObj.pathname + urlObj.search;
    const body = init?.body ? String(init.body) : '';

    // Auto-session path: use existing session if available
    if (this.autoSession && this.hasActiveSession()) {
      const sessionHeaders = this.computeSessionHeaders(method, path, body);
      const sessionInit = this.mergeHeaders(init, sessionHeaders);
      const response = await fetch(url, sessionInit);

      if (!isPaymentRequired(response)) {
        this.sessionBalance -= this.sessionRatePerRequest;
        return response;
      }

      const respBody = await response.clone().json().catch(() => null);
      const error = respBody?.error;
      if (error === 'insufficient_session_balance' || error === 'session_expired_or_not_found') {
        this.invalidateSession();
      } else {
        return response;
      }
    }

    // Auto-session path: no active session — create one on 402
    if (this.autoSession) {
      const response = await fetch(url, this.mergeHeaders(init, {}));

      if (!isPaymentRequired(response)) {
        return response;
      }

      const parsed = await parse402Response(response);
      this.cachedRecipient = parsed.pricing.recipient;
      this.cachedToken = parsed.pricing.token;

      await this.createSession(url);

      const sessionHeaders = this.computeSessionHeaders(method, path, body);
      const sessionInit = this.mergeHeaders(init, sessionHeaders);
      const retryResp = await fetch(url, sessionInit);

      if (!isPaymentRequired(retryResp)) {
        this.sessionBalance -= this.sessionRatePerRequest;
      }
      return retryResp;
    }

    // Direct payment path (autoSession disabled)
    const response = await fetch(url, this.mergeHeaders(init, {}));

    if (!isPaymentRequired(response)) {
      return response;
    }

    const parsed = await parse402Response(response);
    const pricing = parsed.pricing;

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
      const retryInit = this.mergeHeaders(init, paymentHeaders);
      lastResponse = await fetch(url, retryInit);

      if (!isPaymentRequired(lastResponse)) {
        return lastResponse;
      }
    }

    throw new Error(
      `Payment was sent (tx: ${txHash}) but gateway still returned 402 after ${this.maxRetries} retry(s)`
    );
  }

  private async createSession(gatewayUrl: string): Promise<void> {
    const baseUrl = new URL(gatewayUrl).origin;

    const nonceHeaders: Record<string, string> = {
      'X-Payment-Payer': this.payerAddress,
    };
    if (this.agentName) {
      nonceHeaders['X-Payment-Agent'] = this.agentName;
    }

    const nonceResp = await fetch(`${baseUrl}/paygate/sessions/nonce`, {
      method: 'POST',
      headers: nonceHeaders,
    });
    if (!nonceResp.ok) {
      throw new Error(`Session nonce request failed: ${nonceResp.status}`);
    }
    const { nonce } = await nonceResp.json() as { nonce: string };

    const memo = sessionMemo(nonce);
    const decimals = 6;
    const depositBase = Math.round(parseFloat(this.sessionDeposit) * (10 ** decimals));

    const txHash = await this.payFunction({
      to: this.cachedRecipient!,
      amount: BigInt(depositBase),
      token: this.cachedToken!,
      memo,
    });

    const sessionHeaders: Record<string, string> = {
      'X-Payment-Tx': txHash,
      'X-Payment-Payer': this.payerAddress,
      'Content-Type': 'application/json',
    };
    if (this.agentName) {
      sessionHeaders['X-Payment-Agent'] = this.agentName;
    }

    const sessionResp = await fetch(`${baseUrl}/paygate/sessions`, {
      method: 'POST',
      headers: sessionHeaders,
      body: JSON.stringify({ nonce }),
    });

    if (!sessionResp.ok) {
      throw new Error(`Session creation failed: ${sessionResp.status}`);
    }

    const session = await sessionResp.json() as {
      sessionId: string;
      sessionSecret: string;
      balance: string;
      ratePerRequest: string;
    };
    this.sessionId = session.sessionId;
    this.sessionSecret = session.sessionSecret;
    this.sessionBalance = Math.round(parseFloat(session.balance) * 1_000_000);
    this.sessionRatePerRequest = Math.round(parseFloat(session.ratePerRequest) * 1_000_000);
    this.gatewayBaseUrl = baseUrl;
  }

  private computeSessionHeaders(method: string, path: string, body: string): Record<string, string> {
    const timestamp = Math.floor(Date.now() / 1000).toString();
    const reqHash = requestHash(method, path, body);
    const sigPayload = reqHash + timestamp;
    const sig = hmacSha256(this.sessionSecret!, sigPayload);

    return {
      'X-Payment-Session': this.sessionId!,
      'X-Payment-Session-Sig': sig,
      'X-Payment-Timestamp': timestamp,
    };
  }

  private hasActiveSession(): boolean {
    return this.sessionId !== null && this.sessionBalance > this.sessionRatePerRequest;
  }

  private invalidateSession(): void {
    this.sessionId = null;
    this.sessionSecret = null;
    this.sessionBalance = 0;
  }

  private mergeHeaders(init: RequestInit | undefined, extra: Record<string, string>): RequestInit {
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
    const merged = { ...existingHeaders, ...extra };
    if (this.agentName) {
      merged['X-Payment-Agent'] = this.agentName;
    }
    return { ...init, headers: merged };
  }

  private isNetworkError(error: unknown): boolean {
    if (error instanceof TypeError) return true;
    if (typeof DOMException !== 'undefined' && error instanceof DOMException && error.name === 'AbortError') return true;
    if (error instanceof Error) {
      const msg = error.message.toLowerCase();
      if (msg.includes('econnrefused') || msg.includes('enotfound') || msg.includes('etimedout')) {
        return true;
      }
    }
    return false;
  }

  private async bypassToUpstream(url: string, init?: RequestInit): Promise<Response> {
    const original = new URL(url);
    const upstream = new URL(this.upstreamUrl!);
    const bypassUrl = `${upstream.origin}${original.pathname}${original.search}`;

    const cleanInit = { ...init };
    if (cleanInit.headers) {
      const cleaned: Record<string, string> = {};
      if (cleanInit.headers instanceof Headers) {
        cleanInit.headers.forEach((v, k) => { if (!k.toLowerCase().startsWith('x-payment-')) cleaned[k] = v; });
      } else if (Array.isArray(cleanInit.headers)) {
        for (const [k, v] of cleanInit.headers) { if (!k.toLowerCase().startsWith('x-payment-')) cleaned[k] = v; }
      } else {
        for (const [k, v] of Object.entries(cleanInit.headers)) {
          if (!k.toLowerCase().startsWith('x-payment-')) cleaned[k] = v;
        }
      }
      cleanInit.headers = cleaned;
    }

    return fetch(bypassUrl, cleanInit);
  }

  private async getOrFetchPricing(gatewayBaseUrl: string): Promise<Map<string, EndpointPricing>> {
    const now = Date.now();
    if (this.pricingCache && now < this.pricingCacheExpiry) {
      return this.pricingCache;
    }

    this.pricingCache = await fetchEndpointPricing(gatewayBaseUrl);
    this.pricingCacheExpiry = now + PayGateClient.PRICING_CACHE_TTL_MS;
    return this.pricingCache;
  }
}
