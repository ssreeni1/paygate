import { requestHash, paymentMemo, sessionMemo, hmacSha256 } from './hash.js';
import { parse402Response, isPaymentRequired } from './discovery.js';
import { getPricing } from './discovery.js';
import type { PayGateClientOptions, PaymentParams, PricingInfo } from './types.js';

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

  constructor(options: PayGateClientOptions) {
    this.payFunction = options.payFunction;
    this.payerAddress = options.payerAddress;
    this.maxRetries = options.maxRetries ?? 1;
    this.autoSession = options.autoSession ?? false;
    this.sessionDeposit = options.sessionDeposit ?? '0.10';
  }

  /**
   * Fetch a PayGate-protected URL. Handles 402 -> pay -> retry automatically.
   * When autoSession is enabled, manages session lifecycle transparently.
   */
  async fetch(url: string, init?: RequestInit): Promise<Response> {
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

      // Session rejected — check if it's an exhaustion/expiry error
      const respBody = await response.clone().json().catch(() => null);
      const error = respBody?.error;
      if (error === 'insufficient_session_balance' || error === 'session_expired_or_not_found') {
        this.invalidateSession();
        // Fall through to create new session below
      } else {
        return response;
      }
    }

    // Auto-session path: no active session — create one on 402
    if (this.autoSession) {
      const response = await fetch(url, init);

      if (!isPaymentRequired(response)) {
        return response;
      }

      // Parse 402 to get pricing info for session deposit
      const parsed = await parse402Response(response);
      this.cachedRecipient = parsed.pricing.recipient;
      this.cachedToken = parsed.pricing.token;

      // Create session
      await this.createSession(url);

      // Retry with session auth
      const sessionHeaders = this.computeSessionHeaders(method, path, body);
      const sessionInit = this.mergeHeaders(init, sessionHeaders);
      const retryResp = await fetch(url, sessionInit);

      if (!isPaymentRequired(retryResp)) {
        this.sessionBalance -= this.sessionRatePerRequest;
      }
      return retryResp;
    }

    // Direct payment path (autoSession disabled)
    const response = await fetch(url, init);

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

  /**
   * Discover pricing without paying.
   */
  async getPricing(baseUrl: string): Promise<Record<string, PricingInfo>> {
    return getPricing(baseUrl);
  }

  private async createSession(gatewayUrl: string): Promise<void> {
    const baseUrl = new URL(gatewayUrl).origin;

    // Step 1: Get nonce
    const nonceResp = await fetch(`${baseUrl}/paygate/sessions/nonce`, {
      method: 'POST',
      headers: { 'X-Payment-Payer': this.payerAddress },
    });
    if (!nonceResp.ok) {
      throw new Error(`Session nonce request failed: ${nonceResp.status}`);
    }
    const { nonce } = await nonceResp.json() as { nonce: string };

    // Step 2: Send deposit
    const memo = sessionMemo(nonce);
    const decimals = 6; // USDC
    const depositBase = Math.round(parseFloat(this.sessionDeposit) * (10 ** decimals));

    const txHash = await this.payFunction({
      to: this.cachedRecipient!,
      amount: BigInt(depositBase),
      token: this.cachedToken!,
      memo,
    });

    // Step 3: Create session
    const sessionResp = await fetch(`${baseUrl}/paygate/sessions`, {
      method: 'POST',
      headers: {
        'X-Payment-Tx': txHash,
        'X-Payment-Payer': this.payerAddress,
        'Content-Type': 'application/json',
      },
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
    return {
      ...init,
      headers: { ...existingHeaders, ...extra },
    };
  }
}
