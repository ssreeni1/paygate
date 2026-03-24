import { describe, it, expect } from 'vitest';
import { handleTrace } from '../src/tools/trace.js';
import type { ActiveTrace } from '../src/types.js';

describe('paygate_trace', () => {
  it('starts a trace', async () => {
    const traces = new Map<string, ActiveTrace>();
    const handler = handleTrace(traces);
    const result = await handler({ action: 'start', name: 'test-trace' });
    const parsed = JSON.parse(result.content[0].text);
    expect(parsed.status).toBe('started');
    expect(traces.has('test-trace')).toBe(true);
  });

  it('stops a trace and returns summary', async () => {
    const traces = new Map<string, ActiveTrace>();
    traces.set('test-trace', {
      name: 'test-trace',
      startedAt: Date.now() - 5000,
      entries: [
        { endpoint: 'POST /v1/search', method: 'POST', cost: 1000, timestamp: Date.now(), explorerLink: 'https://testnet.tempo.xyz/tx/0x123' },
        { endpoint: 'POST /v1/search', method: 'POST', cost: 1000, timestamp: Date.now(), explorerLink: 'https://testnet.tempo.xyz/tx/0x456' },
      ],
    });

    const handler = handleTrace(traces);
    const result = await handler({ action: 'stop', name: 'test-trace' });
    const parsed = JSON.parse(result.content[0].text);
    expect(parsed.status).toBe('stopped');
    expect(parsed.totalCost).toBe('$0.002000');
    expect(parsed.callCount).toBe(2);
    expect(parsed.breakdown).toHaveLength(1);
    expect(parsed.explorerLinks).toHaveLength(2);
    expect(traces.has('test-trace')).toBe(false);
  });

  it('rejects starting a duplicate trace', async () => {
    const traces = new Map<string, ActiveTrace>();
    traces.set('existing', { name: 'existing', startedAt: Date.now(), entries: [] });
    const handler = handleTrace(traces);
    const result = await handler({ action: 'start', name: 'existing' });
    expect(result.isError).toBe(true);
  });

  it('rejects stopping a non-existent trace', async () => {
    const traces = new Map<string, ActiveTrace>();
    const handler = handleTrace(traces);
    const result = await handler({ action: 'stop', name: 'nope' });
    expect(result.isError).toBe(true);
  });
});
