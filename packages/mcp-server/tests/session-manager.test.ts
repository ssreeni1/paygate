import { describe, it, expect, vi, beforeEach } from 'vitest';
import { SessionManager } from '../src/session-manager.js';
import type { McpServerConfig, SessionState } from '../src/types.js';

describe('SessionManager', () => {
  let manager: SessionManager;
  const mockConfig: McpServerConfig = {
    gatewayUrl: 'https://example.com',
    privateKey: '0x' + 'ab'.repeat(32),
    payerAddress: '0x1234567890abcdef1234567890abcdef12345678',
    agentName: 'test-agent',
    sessionDeposit: '0.10',
    spendLimitDaily: null,
    spendLimitMonthly: null,
  };

  beforeEach(() => {
    const mockClient = {} as any;
    manager = new SessionManager(mockClient, mockConfig);
  });

  it('returns null when no session', () => {
    expect(manager.getSession()).toBeNull();
    expect(manager.getBalance()).toBe(0);
  });

  it('returns session when set', () => {
    const session: SessionState = {
      sessionId: 'sess_abc',
      sessionSecret: 'ssec_def',
      balance: 100_000,
      ratePerRequest: 1_000,
      expiresAt: new Date(Date.now() + 3600_000).toISOString(),
      gatewayBaseUrl: 'https://example.com',
    };
    manager.setSession(session);
    expect(manager.getSession()).not.toBeNull();
    expect(manager.getBalance()).toBe(100_000);
  });

  it('returns null for expired session', () => {
    manager.setSession({
      sessionId: 'sess_expired',
      sessionSecret: 'ssec_def',
      balance: 100_000,
      ratePerRequest: 1_000,
      expiresAt: new Date(Date.now() - 1000).toISOString(),
      gatewayBaseUrl: 'https://example.com',
    });
    expect(manager.getSession()).toBeNull();
  });

  it('deducts balance correctly', () => {
    manager.setSession({
      sessionId: 'sess_abc',
      sessionSecret: 'ssec_def',
      balance: 100_000,
      ratePerRequest: 1_000,
      expiresAt: new Date(Date.now() + 3600_000).toISOString(),
      gatewayBaseUrl: 'https://example.com',
    });
    manager.deductBalance(5_000);
    expect(manager.getBalance()).toBe(95_000);
  });

  it('logs session state on shutdown', () => {
    const stderrSpy = vi.spyOn(process.stderr, 'write').mockImplementation(() => true);
    manager.setSession({
      sessionId: 'sess_abc',
      sessionSecret: 'ssec_def',
      balance: 32_000,
      ratePerRequest: 1_000,
      expiresAt: '2026-03-25T12:00:00Z',
      gatewayBaseUrl: 'https://example.com',
    });
    manager.logShutdownState();
    expect(stderrSpy).toHaveBeenCalledWith(expect.stringContaining('sess_abc'));
    expect(stderrSpy).toHaveBeenCalledWith(expect.stringContaining('$0.032000'));
    stderrSpy.mockRestore();
  });

  it('logs "no active session" when none exists', () => {
    const stderrSpy = vi.spyOn(process.stderr, 'write').mockImplementation(() => true);
    manager.logShutdownState();
    expect(stderrSpy).toHaveBeenCalledWith(expect.stringContaining('No active session'));
    stderrSpy.mockRestore();
  });
});
