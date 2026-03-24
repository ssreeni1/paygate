import { formatUsd } from '../spend-tracker.js';
import { invalidInput, errorToMcpContent } from '../errors.js';
export function handleTrace(activeTraces) {
    return async (input) => {
        if (!input.action || !input.name) {
            return errorToMcpContent(invalidInput('action and name are required'));
        }
        if (input.action === 'start') {
            if (activeTraces.has(input.name)) {
                return errorToMcpContent(invalidInput(`Trace "${input.name}" is already active. Stop it first.`));
            }
            activeTraces.set(input.name, {
                name: input.name,
                startedAt: Date.now(),
                entries: [],
            });
            return {
                content: [{
                        type: 'text',
                        text: JSON.stringify({
                            status: 'started',
                            name: input.name,
                            message: `Trace "${input.name}" started. All paygate_call invocations will be tracked until you stop this trace.`,
                        }, null, 2),
                    }],
            };
        }
        if (input.action === 'stop') {
            const trace = activeTraces.get(input.name);
            if (!trace) {
                return errorToMcpContent(invalidInput(`No active trace named "${input.name}". Start one first.`));
            }
            activeTraces.delete(input.name);
            const totalCost = trace.entries.reduce((sum, e) => sum + e.cost, 0);
            const durationMs = Date.now() - trace.startedAt;
            const byEndpoint = new Map();
            for (const entry of trace.entries) {
                const existing = byEndpoint.get(entry.endpoint) ?? { count: 0, totalCost: 0 };
                existing.count += 1;
                existing.totalCost += entry.cost;
                byEndpoint.set(entry.endpoint, existing);
            }
            const breakdown = Array.from(byEndpoint.entries()).map(([endpoint, data]) => ({
                endpoint,
                calls: data.count,
                totalCost: formatUsd(data.totalCost),
            }));
            return {
                content: [{
                        type: 'text',
                        text: JSON.stringify({
                            status: 'stopped',
                            name: input.name,
                            totalCost: formatUsd(totalCost),
                            callCount: trace.entries.length,
                            durationMs,
                            breakdown,
                            explorerLinks: trace.entries
                                .filter((e) => e.explorerLink)
                                .map((e) => e.explorerLink),
                        }, null, 2),
                    }],
            };
        }
        return errorToMcpContent(invalidInput(`Unknown action: ${input.action}. Use "start" or "stop".`));
    };
}
