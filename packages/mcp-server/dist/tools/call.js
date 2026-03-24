import { formatUsd } from '../spend-tracker.js';
import { classifyError, errorToMcpContent, spendLimitExceeded, invalidInput, upstreamError } from '../errors.js';
export function handleCall(client, config, spendTracker, sessionManager, pricingCache, activeTraces) {
    return async (input) => {
        if (!input.method || !input.path) {
            return errorToMcpContent(invalidInput('method and path are required'));
        }
        const method = input.method.toUpperCase();
        if (!['GET', 'POST', 'PUT', 'DELETE'].includes(method)) {
            return errorToMcpContent(invalidInput(`Unsupported method: ${method}`));
        }
        const endpoint = `${method} ${input.path}`;
        const epPricing = await pricingCache.priceFor(endpoint);
        const estimatedCost = epPricing?.priceBaseUnits ?? 0;
        const limitViolation = spendTracker.checkLimit(estimatedCost);
        if (limitViolation) {
            return errorToMcpContent(spendLimitExceeded(formatUsd(limitViolation.spent), formatUsd(limitViolation.limit), limitViolation.period));
        }
        try {
            const url = `${config.gatewayUrl}${input.path}`;
            const requestInit = {
                method,
                ...(input.body ? { body: JSON.stringify(input.body) } : {}),
                headers: {
                    'Content-Type': 'application/json',
                    ...(config.agentName ? { 'X-Payment-Agent': config.agentName } : {}),
                    ...(input.headers ?? {}),
                },
            };
            const response = await client.fetch(url, requestInit);
            const costHeader = response.headers.get('X-Payment-Cost');
            const costBaseUnits = costHeader ? Math.round(parseFloat(costHeader) * 1_000_000) : estimatedCost;
            const txHash = response.headers.get('X-Payment-Tx');
            const balanceHeader = response.headers.get('X-Payment-Balance');
            spendTracker.record_spend(costBaseUnits);
            sessionManager.updateFromSdkResponse(response.headers);
            const explorerLink = txHash
                ? `https://testnet.tempo.xyz/tx/${txHash}`
                : null;
            process.stderr.write(`[paygate] ${endpoint} — cost: ${formatUsd(costBaseUnits)}` +
                (explorerLink ? ` — ${explorerLink}` : '') + '\n');
            for (const trace of activeTraces.values()) {
                trace.entries.push({
                    endpoint,
                    method,
                    cost: costBaseUnits,
                    timestamp: Date.now(),
                    explorerLink: explorerLink ?? '',
                });
            }
            const responseBody = await response.text();
            let parsedBody;
            try {
                parsedBody = JSON.parse(responseBody);
            }
            catch {
                parsedBody = responseBody;
            }
            if (response.status >= 500) {
                const refunded = response.headers.get('X-Payment-Refunded') === 'true';
                return errorToMcpContent(upstreamError(response.status, `${responseBody.slice(0, 200)}${refunded ? ' (payment refunded)' : ''}`));
            }
            if (response.status >= 400) {
                return {
                    content: [{
                            type: 'text',
                            text: JSON.stringify({
                                status: response.status,
                                body: parsedBody,
                            }, null, 2),
                        }],
                    isError: true,
                };
            }
            const balanceRemaining = balanceHeader
                ? `$${parseFloat(balanceHeader).toFixed(6)}`
                : formatUsd(sessionManager.getBalance());
            return {
                content: [{
                        type: 'text',
                        text: JSON.stringify({
                            result: parsedBody,
                            payment: {
                                cost: formatUsd(costBaseUnits),
                                explorerLink: explorerLink ?? 'N/A',
                                balanceRemaining,
                            },
                        }, null, 2),
                    }],
            };
        }
        catch (err) {
            return errorToMcpContent(classifyError(err));
        }
    };
}
