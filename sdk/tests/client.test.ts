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
});
