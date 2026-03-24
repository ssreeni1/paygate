import { existsSync, readFileSync, writeFileSync, mkdirSync } from 'fs';
import { join } from 'path';
import { homedir } from 'os';
import type { SessionState, McpServerConfig } from './types.js';

export class SessionManager {
  private session: SessionState | null = null;
  private config: McpServerConfig;
  private secretsDir: string;

  constructor(client: unknown, config: McpServerConfig) {
    this.config = config;
    this.secretsDir = join(homedir(), '.paygate', 'sessions');
    try { mkdirSync(this.secretsDir, { recursive: true }); } catch {}
  }

  /** Persist session secret to disk for resume after restart */
  persistSecret(sessionId: string, secret: string): void {
    try {
      writeFileSync(join(this.secretsDir, sessionId), secret, { mode: 0o600 });
    } catch {}
  }

  /** Load persisted secret for a session */
  loadPersistedSecret(sessionId: string): string | null {
    try {
      const path = join(this.secretsDir, sessionId);
      if (!existsSync(path)) return null;
      return readFileSync(path, 'utf-8').trim();
    } catch {
      return null;
    }
  }

  getSession(): SessionState | null {
    if (!this.session) return null;
    if (new Date(this.session.expiresAt).getTime() < Date.now()) {
      this.session = null;
      return null;
    }
    return this.session;
  }

  getBalance(): number {
    return this.session?.balance ?? 0;
  }

  deductBalance(amount: number): void {
    if (this.session) {
      this.session.balance = Math.max(0, this.session.balance - amount);
    }
  }

  updateFromSdkResponse(responseHeaders: Headers): void {
    const cost = responseHeaders.get('X-Payment-Cost');
    if (cost) {
      const costBaseUnits = Math.round(parseFloat(cost) * 1_000_000);
      this.deductBalance(costBaseUnits);
    }
    const balance = responseHeaders.get('X-Payment-Balance');
    if (balance && this.session) {
      this.session.balance = Math.round(parseFloat(balance) * 1_000_000);
    }
  }

  async tryResumeSession(): Promise<boolean> {
    try {
      const resp = await fetch(
        `${this.config.gatewayUrl}/paygate/sessions?payer=${this.config.payerAddress}`,
        { signal: AbortSignal.timeout(5_000) }
      );
      if (!resp.ok) return false;

      const body = await resp.json() as {
        sessions: Array<{
          sessionId: string;
          balance: string;
          ratePerRequest: string;
          expiresAt: string;
          status: string;
        }>;
      };

      const active = body.sessions
        .filter((s) => s.status === 'active')
        .filter((s) => new Date(s.expiresAt).getTime() > Date.now())
        .sort((a, b) => new Date(b.expiresAt).getTime() - new Date(a.expiresAt).getTime());

      if (active.length === 0) return false;

      const best = active[0];
      const balance = Math.round(parseFloat(best.balance) * 1_000_000);
      const rate = Math.round(parseFloat(best.ratePerRequest) * 1_000_000);

      if (balance < rate) return false;

      // Check if we have a persisted secret for this session
      const persistedSecret = this.loadPersistedSecret(best.sessionId);
      if (persistedSecret) {
        this.session = {
          sessionId: best.sessionId,
          sessionSecret: persistedSecret,
          balance,
          ratePerRequest: rate,
          expiresAt: new Date(best.expiresAt).getTime(),
        };
        process.stderr.write(
          `[paygate] Resumed session ${best.sessionId} with $${(balance / 1_000_000).toFixed(6)} remaining.\n`
        );
        return true;
      }

      process.stderr.write(
        `[paygate] Found active session ${best.sessionId} with $${(balance / 1_000_000).toFixed(6)} remaining — ` +
        `no persisted secret, will create new session on first call.\n`
      );
      return false;
    } catch {
      process.stderr.write('[paygate] Could not check for active sessions on startup.\n');
      return false;
    }
  }

  setSession(state: SessionState): void {
    this.session = state;
    // Persist secret to disk so we can resume after restart
    if (state.sessionSecret) {
      this.persistSecret(state.sessionId, state.sessionSecret);
    }
  }

  invalidate(): void {
    this.session = null;
  }

  logShutdownState(): void {
    if (this.session) {
      const balance = (this.session.balance / 1_000_000).toFixed(6);
      process.stderr.write(
        `[paygate] Session ${this.session.sessionId} has $${balance} remaining — ` +
        `expires at ${this.session.expiresAt}\n`
      );
    } else {
      process.stderr.write('[paygate] No active session at shutdown.\n');
    }
  }
}
