export class SessionManager {
    session = null;
    config;
    constructor(client, config) {
        this.config = config;
    }
    getSession() {
        if (!this.session)
            return null;
        if (new Date(this.session.expiresAt).getTime() < Date.now()) {
            this.session = null;
            return null;
        }
        return this.session;
    }
    getBalance() {
        return this.session?.balance ?? 0;
    }
    deductBalance(amount) {
        if (this.session) {
            this.session.balance = Math.max(0, this.session.balance - amount);
        }
    }
    updateFromSdkResponse(responseHeaders) {
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
    async tryResumeSession() {
        try {
            const resp = await fetch(`${this.config.gatewayUrl}/paygate/sessions?payer=${this.config.payerAddress}`, { signal: AbortSignal.timeout(5_000) });
            if (!resp.ok)
                return false;
            const body = await resp.json();
            const active = body.sessions
                .filter((s) => s.status === 'active')
                .filter((s) => new Date(s.expiresAt).getTime() > Date.now())
                .sort((a, b) => new Date(b.expiresAt).getTime() - new Date(a.expiresAt).getTime());
            if (active.length === 0)
                return false;
            const best = active[0];
            const balance = Math.round(parseFloat(best.balance) * 1_000_000);
            const rate = Math.round(parseFloat(best.ratePerRequest) * 1_000_000);
            if (balance < rate)
                return false;
            process.stderr.write(`[paygate] Found active session ${best.sessionId} with $${(balance / 1_000_000).toFixed(6)} remaining — ` +
                `expires ${best.expiresAt}. SDK will reuse if secret is cached.\n`);
            return false;
        }
        catch {
            process.stderr.write('[paygate] Could not check for active sessions on startup.\n');
            return false;
        }
    }
    setSession(state) {
        this.session = state;
    }
    invalidate() {
        this.session = null;
    }
    logShutdownState() {
        if (this.session) {
            const balance = (this.session.balance / 1_000_000).toFixed(6);
            process.stderr.write(`[paygate] Session ${this.session.sessionId} has $${balance} remaining — ` +
                `expires at ${this.session.expiresAt}\n`);
        }
        else {
            process.stderr.write('[paygate] No active session at shutdown.\n');
        }
    }
}
