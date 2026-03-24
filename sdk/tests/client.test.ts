import { describe, it, expect, vi, beforeEach } from 'vitest';
import { PayGateClient } from '../src/client.js';
import type { PaymentParams } from '../src/types.js';

const MOCK_402_BODY = {
  error: 'payment_required',
  message: 'Send 0.005000 USDC to 0x7F3aProvider on Tempo, then retry with X-Payment-Tx header.',
  help_url: 'https://ssreeni1.github.io/paygate/quickstart#paying',
  pricing: {
    amount: '0.005000',
    amount_base_units: 5000,
    decimals: 6,
    token: '0xUSDC',
    recipient: '0xProvider',
    quote_id: 'qt_test123',
    quote_expires_at: '2026-03-18T12:00:00Z',
    methods: ['direct', 'session'],
  },
};

function make402Response(): Response {
  return new Response(JSON.stringify(MOCK_402_BODY), {
    status: 402,
    headers: { 'Content-Type': 'application/json' },
  });
}

function make200Response(body: string = '{"ok":true}'): Response {
  return new Response(body, {
    status: 200,
    headers: { 'Content-Type': 'application/json' },
  });
}

describe('PayGateClient', () => {
  let mockPayFunction: ReturnType<typeof vi.fn>;
  let client: PayGateClient;

  beforeEach(() => {
    vi.restoreAllMocks();
    mockPayFunction = vi.fn().mockResolvedValue('0xtxhash123');
    client = new PayGateClient({
      payFunction: mockPayFunction,
      payerAddress: '0xPayer',
    });
  });

  it('passes through free endpoint (200 response)', async () => {
    const mockFetch = vi.spyOn(globalThis, 'fetch').mockResolvedValueOnce(make200Response());

    const response = await client.fetch('https://api.example.com/v1/models');

    expect(response.status).toBe(200);
    expect(mockPayFunction).not.toHaveBeenCalled();
    expect(mockFetch).toHaveBeenCalledTimes(1);
  });

  it('handles 402 -> pay -> 200 auto-pay flow', async () => {
    const mockFetch = vi.spyOn(globalThis, 'fetch')
      .mockResolvedValueOnce(make402Response())
      .mockResolvedValueOnce(make200Response('{"result":"paid"}'));

    const response = await client.fetch('https://api.example.com/v1/chat/completions', {
      method: 'POST',
      body: JSON.stringify({ model: 'gpt-4' }),
    });

    expect(response.status).toBe(200);
    const body = await response.json();
    expect(body).toEqual({ result: 'paid' });

    // Verify payment was called with correct params
    expect(mockPayFunction).toHaveBeenCalledOnce();
    const payParams = mockPayFunction.mock.calls[0][0];
    expect(payParams.to).toBe('0xProvider');
    expect(payParams.amount).toBe(5000n);
    expect(payParams.token).toBe('0xUSDC');
    expect(payParams.memo).toMatch(/^0x[a-f0-9]{64}$/);

    // Verify retry included payment headers
    expect(mockFetch).toHaveBeenCalledTimes(2);
    const retryCall = mockFetch.mock.calls[1];
    const retryInit = retryCall[1] as RequestInit;
    const headers = retryInit.headers as Record<string, string>;
    expect(headers['X-Payment-Tx']).toBe('0xtxhash123');
    expect(headers['X-Payment-Payer']).toBe('0xPayer');
    expect(headers['X-Payment-Quote-Id']).toBe('qt_test123');
  });

  it('throws on payment failure', async () => {
    vi.spyOn(globalThis, 'fetch').mockResolvedValueOnce(make402Response());
    mockPayFunction.mockRejectedValueOnce(new Error('insufficient funds'));

    await expect(
      client.fetch('https://api.example.com/v1/chat', {
        method: 'POST',
        body: '{}',
      })
    ).rejects.toThrow('insufficient funds');
  });

  it('throws after retry exhaustion', async () => {
    vi.spyOn(globalThis, 'fetch')
      .mockResolvedValueOnce(make402Response())
      .mockResolvedValueOnce(make402Response());

    await expect(
      client.fetch('https://api.example.com/v1/chat', {
        method: 'POST',
        body: '{}',
      })
    ).rejects.toThrow(/still returned 402 after 1 retry/);
  });

  it('respects maxRetries option', async () => {
    const clientWith3Retries = new PayGateClient({
      payFunction: mockPayFunction,
      payerAddress: '0xPayer',
      maxRetries: 3,
    });

    const mockFetch = vi.spyOn(globalThis, 'fetch')
      .mockResolvedValueOnce(make402Response())
      .mockResolvedValueOnce(make402Response())
      .mockResolvedValueOnce(make402Response())
      .mockResolvedValueOnce(make200Response());

    const response = await clientWith3Retries.fetch('https://api.example.com/v1/chat', {
      method: 'POST',
      body: '{}',
    });

    expect(response.status).toBe(200);
    // 1 initial + 3 retries
    expect(mockFetch).toHaveBeenCalledTimes(4);
  });

  it('uses GET method and empty body when no init provided', async () => {
    vi.spyOn(globalThis, 'fetch')
      .mockResolvedValueOnce(make402Response())
      .mockResolvedValueOnce(make200Response());

    const response = await client.fetch('https://api.example.com/v1/models');

    expect(response.status).toBe(200);
    expect(mockPayFunction).toHaveBeenCalledOnce();
  });

  describe('auto-session', () => {
    const MOCK_NONCE_RESPONSE = () =>
      new Response(JSON.stringify({ nonce: 'nonce_abc' }), {
        status: 200,
        headers: { 'Content-Type': 'application/json' },
      });

    const MOCK_SESSION_CREATE_RESPONSE = () =>
      new Response(
        JSON.stringify({
          sessionId: 'sess_1',
          sessionSecret: 'ssec_aabbccdd',
          balance: '0.050000',
          ratePerRequest: '0.000500',
        }),
        { status: 200, headers: { 'Content-Type': 'application/json' } },
      );

    const MOCK_SESSION_EXHAUSTED_RESPONSE = () =>
      new Response(
        JSON.stringify({
          error: 'insufficient_session_balance',
          message: 'Session balance exhausted',
        }),
        { status: 402, headers: { 'Content-Type': 'application/json' } },
      );

    it('auto-session first call creates session', async () => {
      const sessionClient = new PayGateClient({
        payFunction: mockPayFunction,
        payerAddress: '0xPayer',
        autoSession: true,
        sessionDeposit: '0.05',
      });

      const mockFetch = vi.spyOn(globalThis, 'fetch')
        .mockResolvedValueOnce(make402Response())       // 1. initial request -> 402
        .mockResolvedValueOnce(MOCK_NONCE_RESPONSE())   // 2. nonce endpoint
        .mockResolvedValueOnce(MOCK_SESSION_CREATE_RESPONSE()) // 3. session create
        .mockResolvedValueOnce(make200Response());       // 4. retry with session headers

      const response = await sessionClient.fetch('https://api.example.com/v1/chat/completions', {
        method: 'POST',
        body: JSON.stringify({ model: 'gpt-4' }),
      });

      // payFunction was called for the session deposit
      expect(mockPayFunction).toHaveBeenCalledOnce();

      // Final response is 200
      expect(response.status).toBe(200);

      // fetch was called 4 times
      expect(mockFetch).toHaveBeenCalledTimes(4);

      // The 4th call (retry) has session auth headers
      const retryCall = mockFetch.mock.calls[3];
      const retryInit = retryCall[1] as RequestInit;
      const headers = retryInit.headers as Record<string, string>;
      expect(headers['X-Payment-Session']).toBe('sess_1');
      expect(headers['X-Payment-Session-Sig']).toBeDefined();
      expect(headers['X-Payment-Session-Sig']).toMatch(/^[a-f0-9]+$/);
      expect(headers['X-Payment-Timestamp']).toBeDefined();
      expect(headers['X-Payment-Timestamp']).toMatch(/^\d+$/);
    });

    it('auto-session subsequent call uses HMAC without new payment', async () => {
      const sessionClient = new PayGateClient({
        payFunction: mockPayFunction,
        payerAddress: '0xPayer',
        autoSession: true,
        sessionDeposit: '0.05',
      });

      // First call: creates session (4 fetch calls)
      const mockFetch = vi.spyOn(globalThis, 'fetch')
        .mockResolvedValueOnce(make402Response())
        .mockResolvedValueOnce(MOCK_NONCE_RESPONSE())
        .mockResolvedValueOnce(MOCK_SESSION_CREATE_RESPONSE())
        .mockResolvedValueOnce(make200Response());

      await sessionClient.fetch('https://api.example.com/v1/chat/completions', {
        method: 'POST',
        body: JSON.stringify({ model: 'gpt-4' }),
      });

      expect(mockPayFunction).toHaveBeenCalledOnce();

      // Second call: should use existing session (1 fetch call, no payment)
      mockFetch.mockResolvedValueOnce(make200Response('{"result":"second"}'));

      const response2 = await sessionClient.fetch('https://api.example.com/v1/chat/completions', {
        method: 'POST',
        body: JSON.stringify({ model: 'gpt-4', prompt: 'hello' }),
      });

      // payFunction was NOT called again
      expect(mockPayFunction).toHaveBeenCalledOnce();

      // The 5th fetch call (second request) has session headers
      expect(mockFetch).toHaveBeenCalledTimes(5);
      const secondCall = mockFetch.mock.calls[4];
      const secondInit = secondCall[1] as RequestInit;
      const headers = secondInit.headers as Record<string, string>;
      expect(headers['X-Payment-Session']).toBe('sess_1');
      expect(headers['X-Payment-Session-Sig']).toBeDefined();
      expect(headers['X-Payment-Timestamp']).toBeDefined();

      expect(response2.status).toBe(200);
    });

    it('session exhausted triggers auto-renew', async () => {
      const sessionClient = new PayGateClient({
        payFunction: mockPayFunction,
        payerAddress: '0xPayer',
        autoSession: true,
        sessionDeposit: '0.05',
      });

      // First call: creates initial session
      const mockFetch = vi.spyOn(globalThis, 'fetch')
        .mockResolvedValueOnce(make402Response())
        .mockResolvedValueOnce(MOCK_NONCE_RESPONSE())
        .mockResolvedValueOnce(MOCK_SESSION_CREATE_RESPONSE())
        .mockResolvedValueOnce(make200Response());

      await sessionClient.fetch('https://api.example.com/v1/chat/completions', {
        method: 'POST',
        body: JSON.stringify({ model: 'gpt-4' }),
      });

      expect(mockPayFunction).toHaveBeenCalledOnce();

      // Drain the session balance by setting it to 0 via repeated calls
      // Instead, mock the gateway returning 402 with insufficient_session_balance
      // This simulates the session being exhausted server-side
      mockFetch
        .mockResolvedValueOnce(MOCK_SESSION_EXHAUSTED_RESPONSE()) // 5. session auth rejected
        .mockResolvedValueOnce(make402Response())                  // 6. re-request gets 402 with pricing
        .mockResolvedValueOnce(MOCK_NONCE_RESPONSE())              // 7. new nonce
        .mockResolvedValueOnce(MOCK_SESSION_CREATE_RESPONSE())     // 8. new session create
        .mockResolvedValueOnce(make200Response('{"renewed":true}')); // 9. retry with new session

      const response = await sessionClient.fetch('https://api.example.com/v1/chat/completions', {
        method: 'POST',
        body: JSON.stringify({ model: 'gpt-4' }),
      });

      // payFunction called again for the new deposit
      expect(mockPayFunction).toHaveBeenCalledTimes(2);

      // New session created, final response is 200
      expect(response.status).toBe(200);
    });

    it('non-auto-session mode uses direct payment without session headers', async () => {
      // client from beforeEach has autoSession: false (default)
      const mockFetch = vi.spyOn(globalThis, 'fetch')
        .mockResolvedValueOnce(make402Response())
        .mockResolvedValueOnce(make200Response('{"direct":true}'));

      const response = await client.fetch('https://api.example.com/v1/chat/completions', {
        method: 'POST',
        body: JSON.stringify({ model: 'gpt-4' }),
      });

      expect(response.status).toBe(200);
      expect(mockPayFunction).toHaveBeenCalledOnce();
      expect(mockFetch).toHaveBeenCalledTimes(2);

      // Verify retry has direct payment headers, NOT session headers
      const retryCall = mockFetch.mock.calls[1];
      const retryInit = retryCall[1] as RequestInit;
      const headers = retryInit.headers as Record<string, string>;
      expect(headers['X-Payment-Tx']).toBe('0xtxhash123');
      expect(headers['X-Payment-Payer']).toBe('0xPayer');
      expect(headers['X-Payment-Quote-Id']).toBe('qt_test123');

      // No session headers
      expect(headers['X-Payment-Session']).toBeUndefined();
      expect(headers['X-Payment-Session-Sig']).toBeUndefined();
      expect(headers['X-Payment-Timestamp']).toBeUndefined();
    });
  });
});

// --- Wave 3 Tests ---

const MOCK_PRICING_RESPONSE = {
  apis: [
    { endpoint: 'POST /v1/search', price: '0.005000', price_base_units: 5000, decimals: 6, dynamic: false },
    { endpoint: 'POST /v1/summarize', price: '0.010000', price_base_units: 10000, decimals: 6, dynamic: true },
    { endpoint: 'POST /v1/image', price: '0.050000', price_base_units: 50000, decimals: 6, dynamic: false },
  ],
};

function makePricingResponse(): Response {
  return new Response(JSON.stringify(MOCK_PRICING_RESPONSE), {
    status: 200,
    headers: { 'Content-Type': 'application/json' },
  });
}

describe('estimateCost', () => {
  let mockPayFunction: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    vi.restoreAllMocks();
    mockPayFunction = vi.fn().mockResolvedValue('0xtxhash123');
  });

  it('computes total and breakdown for multiple endpoints', async () => {
    vi.spyOn(globalThis, 'fetch').mockResolvedValueOnce(makePricingResponse());

    const client = new PayGateClient({
      payFunction: mockPayFunction,
      payerAddress: '0xPayer',
    });

    const result = await client.estimateCost('https://gateway.example.com', [
      { endpoint: 'POST /v1/search', count: 3 },
      { endpoint: 'POST /v1/summarize', count: 2 },
    ]);

    expect(result.total).toBe('0.035000');
    expect(result.breakdown).toHaveLength(2);
    expect(result.breakdown[0]).toEqual({
      endpoint: 'POST /v1/search',
      price: '0.005000',
      count: 3,
      subtotal: '0.015000',
      dynamic: false,
    });
    expect(result.breakdown[1]).toEqual({
      endpoint: 'POST /v1/summarize',
      price: '0.010000',
      count: 2,
      subtotal: '0.020000',
      dynamic: true,
    });
    expect(result.withinBudget).toBe(true);
  });

  it('withinBudget is true when total <= spendLimit', async () => {
    vi.spyOn(globalThis, 'fetch').mockResolvedValueOnce(makePricingResponse());

    const client = new PayGateClient({
      payFunction: mockPayFunction,
      payerAddress: '0xPayer',
      spendLimit: '0.05',
    });

    const result = await client.estimateCost('https://gateway.example.com', [
      { endpoint: 'POST /v1/search', count: 5 },
    ]);

    expect(result.total).toBe('0.025000');
    expect(result.withinBudget).toBe(true);
  });

  it('withinBudget is false when total > spendLimit', async () => {
    vi.spyOn(globalThis, 'fetch').mockResolvedValueOnce(makePricingResponse());

    const client = new PayGateClient({
      payFunction: mockPayFunction,
      payerAddress: '0xPayer',
      spendLimit: '0.01',
    });

    const result = await client.estimateCost('https://gateway.example.com', [
      { endpoint: 'POST /v1/search', count: 5 },
    ]);

    expect(result.total).toBe('0.025000');
    expect(result.withinBudget).toBe(false);
  });

  it('uses cached pricing on second call within TTL', async () => {
    const mockFetch = vi.spyOn(globalThis, 'fetch').mockResolvedValueOnce(makePricingResponse());

    const client = new PayGateClient({
      payFunction: mockPayFunction,
      payerAddress: '0xPayer',
    });

    await client.estimateCost('https://gateway.example.com', [
      { endpoint: 'POST /v1/search', count: 1 },
    ]);

    const result = await client.estimateCost('https://gateway.example.com', [
      { endpoint: 'POST /v1/summarize', count: 1 },
    ]);

    expect(mockFetch).toHaveBeenCalledTimes(1);
    expect(result.total).toBe('0.010000');
  });

  it('throws on unknown endpoint', async () => {
    vi.spyOn(globalThis, 'fetch').mockResolvedValueOnce(makePricingResponse());

    const client = new PayGateClient({
      payFunction: mockPayFunction,
      payerAddress: '0xPayer',
    });

    await expect(
      client.estimateCost('https://gateway.example.com', [
        { endpoint: 'POST /v1/nonexistent', count: 1 },
      ])
    ).rejects.toThrow('Unknown endpoint');
  });

  it('returns zero total for empty calls array', async () => {
    vi.spyOn(globalThis, 'fetch').mockResolvedValueOnce(makePricingResponse());

    const client = new PayGateClient({
      payFunction: mockPayFunction,
      payerAddress: '0xPayer',
      spendLimit: '1.00',
    });

    const result = await client.estimateCost('https://gateway.example.com', []);

    expect(result.total).toBe('0.000000');
    expect(result.breakdown).toHaveLength(0);
    expect(result.withinBudget).toBe(true);
  });

  it('marks dynamic-priced endpoints in breakdown', async () => {
    vi.spyOn(globalThis, 'fetch').mockResolvedValueOnce(makePricingResponse());

    const client = new PayGateClient({
      payFunction: mockPayFunction,
      payerAddress: '0xPayer',
    });

    const result = await client.estimateCost('https://gateway.example.com', [
      { endpoint: 'POST /v1/search', count: 1 },
      { endpoint: 'POST /v1/summarize', count: 1 },
    ]);

    const searchEntry = result.breakdown.find(b => b.endpoint === 'POST /v1/search')!;
    const summarizeEntry = result.breakdown.find(b => b.endpoint === 'POST /v1/summarize')!;

    expect(searchEntry.dynamic).toBe(false);
    expect(summarizeEntry.dynamic).toBe(true);
  });
});

describe('failureMode', () => {
  let mockPayFunction: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    vi.restoreAllMocks();
    mockPayFunction = vi.fn().mockResolvedValue('0xtxhash123');
  });

  it('closed (default) throws on network error', async () => {
    vi.spyOn(globalThis, 'fetch').mockRejectedValueOnce(new TypeError('fetch failed'));

    const client = new PayGateClient({
      payFunction: mockPayFunction,
      payerAddress: '0xPayer',
    });

    await expect(
      client.fetch('https://gateway.example.com/v1/search', {
        method: 'POST',
        body: '{"q":"test"}',
      })
    ).rejects.toThrow('fetch failed');
  });

  it('open bypasses to upstream on network error', async () => {
    const mockFetch = vi.spyOn(globalThis, 'fetch')
      .mockRejectedValueOnce(new TypeError('fetch failed'))
      .mockResolvedValueOnce(make200Response('{"fallback":true}'));

    const client = new PayGateClient({
      payFunction: mockPayFunction,
      payerAddress: '0xPayer',
      failureMode: 'open',
      upstreamUrl: 'https://upstream.example.com',
    });

    const response = await client.fetch('https://gateway.example.com/v1/search?q=test', {
      method: 'POST',
      body: '{"q":"test"}',
    });

    expect(response.status).toBe(200);
    const body = await response.json();
    expect(body).toEqual({ fallback: true });

    expect(mockFetch).toHaveBeenCalledTimes(2);
    const bypassCall = mockFetch.mock.calls[1];
    expect(bypassCall[0]).toBe('https://upstream.example.com/v1/search?q=test');
  });

  it('open does NOT bypass on gateway 5xx (only network errors)', async () => {
    vi.spyOn(globalThis, 'fetch').mockResolvedValueOnce(
      new Response('Internal Server Error', { status: 500 })
    );

    const client = new PayGateClient({
      payFunction: mockPayFunction,
      payerAddress: '0xPayer',
      failureMode: 'open',
      upstreamUrl: 'https://upstream.example.com',
    });

    const response = await client.fetch('https://gateway.example.com/v1/search', {
      method: 'POST',
      body: '{}',
    });

    expect(response.status).toBe(500);
  });

  it('open without upstreamUrl throws at construction', () => {
    expect(() => new PayGateClient({
      payFunction: mockPayFunction,
      payerAddress: '0xPayer',
      failureMode: 'open',
    })).toThrow("failureMode 'open' requires upstreamUrl");
  });
});

describe('agentName', () => {
  let mockPayFunction: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    vi.restoreAllMocks();
    mockPayFunction = vi.fn().mockResolvedValue('0xtxhash123');
  });

  it('includes X-Payment-Agent header on every outgoing request', async () => {
    const mockFetch = vi.spyOn(globalThis, 'fetch')
      .mockResolvedValueOnce(make402Response())
      .mockResolvedValueOnce(make200Response());

    const client = new PayGateClient({
      payFunction: mockPayFunction,
      payerAddress: '0xPayer',
      agentName: 'my-research-bot',
    });

    await client.fetch('https://gateway.example.com/v1/search', {
      method: 'POST',
      body: '{"q":"test"}',
    });

    for (const call of mockFetch.mock.calls) {
      const init = call[1] as RequestInit;
      const headers = init.headers as Record<string, string>;
      expect(headers['X-Payment-Agent']).toBe('my-research-bot');
    }
  });

  it('includes X-Payment-Agent in session nonce and creation requests', async () => {
    const sessionClient = new PayGateClient({
      payFunction: mockPayFunction,
      payerAddress: '0xPayer',
      autoSession: true,
      sessionDeposit: '0.05',
      agentName: 'session-bot',
    });

    const mockFetch = vi.spyOn(globalThis, 'fetch')
      .mockResolvedValueOnce(make402Response())
      .mockResolvedValueOnce(
        new Response(JSON.stringify({ nonce: 'nonce_abc' }), {
          status: 200,
          headers: { 'Content-Type': 'application/json' },
        })
      )
      .mockResolvedValueOnce(
        new Response(JSON.stringify({
          sessionId: 'sess_1',
          sessionSecret: 'ssec_aabbccdd',
          balance: '0.050000',
          ratePerRequest: '0.000500',
        }), { status: 200, headers: { 'Content-Type': 'application/json' } })
      )
      .mockResolvedValueOnce(make200Response());

    await sessionClient.fetch('https://gateway.example.com/v1/chat', {
      method: 'POST',
      body: '{}',
    });

    // Check nonce request (call index 1) has agent header
    const nonceInit = mockFetch.mock.calls[1][1] as RequestInit;
    const nonceHeaders = nonceInit.headers as Record<string, string>;
    expect(nonceHeaders['X-Payment-Agent']).toBe('session-bot');

    // Check session creation request (call index 2) has agent header
    const createInit = mockFetch.mock.calls[2][1] as RequestInit;
    const createHeaders = createInit.headers as Record<string, string>;
    expect(createHeaders['X-Payment-Agent']).toBe('session-bot');
  });
});
