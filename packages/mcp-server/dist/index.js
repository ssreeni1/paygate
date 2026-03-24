#!/usr/bin/env node
import { Server } from '@modelcontextprotocol/sdk/server/index.js';
import { StdioServerTransport } from '@modelcontextprotocol/sdk/server/stdio.js';
import { CallToolRequestSchema, ListToolsRequestSchema, } from '@modelcontextprotocol/sdk/types.js';
import { PayGateClient } from '@paygate/sdk';
import { createWalletClient, http } from 'viem';
import { privateKeyToAccount } from 'viem/accounts';
import { loadPrivateKey } from './key-loader.js';
import { PricingCacheManager } from './pricing-cache.js';
import { SpendTracker, parseUsdcToBaseUnits, formatUsd } from './spend-tracker.js';
import { SessionManager } from './session-manager.js';
import { handleDiscover } from './tools/discover.js';
import { handleCall } from './tools/call.js';
import { handleBudget } from './tools/budget.js';
import { handleEstimate } from './tools/estimate.js';
import { handleTrace } from './tools/trace.js';
import { invalidInput, errorToMcpContent } from './errors.js';
async function main() {
    const privateKey = loadPrivateKey();
    const account = privateKeyToAccount(privateKey);
    const gatewayUrl = process.env.PAYGATE_GATEWAY_URL;
    if (!gatewayUrl) {
        throw new Error('PAYGATE_GATEWAY_URL is required');
    }
    const config = {
        gatewayUrl,
        privateKey,
        payerAddress: account.address,
        agentName: process.env.PAYGATE_AGENT_NAME ?? 'mcp-agent',
        sessionDeposit: process.env.PAYGATE_SESSION_DEPOSIT ?? '0.10',
        spendLimitDaily: parseUsdcToBaseUnits(process.env.PAYGATE_SPEND_LIMIT_DAILY),
        spendLimitMonthly: parseUsdcToBaseUnits(process.env.PAYGATE_SPEND_LIMIT_MONTHLY),
    };
    const rpcUrl = process.env.PAYGATE_RPC_URL ?? 'https://rpc.testnet.tempo.xyz';
    const walletClient = createWalletClient({
        account,
        transport: http(rpcUrl),
    });
    const payFunction = async (params) => {
        const txHash = await walletClient.writeContract({
            address: params.token,
            abi: [
                {
                    name: 'transferWithMemo',
                    type: 'function',
                    stateMutability: 'nonpayable',
                    inputs: [
                        { name: 'to', type: 'address' },
                        { name: 'value', type: 'uint256' },
                        { name: 'memo', type: 'bytes32' },
                    ],
                    outputs: [{ type: 'bool' }],
                },
            ],
            functionName: 'transferWithMemo',
            args: [params.to, params.amount, params.memo],
        });
        return txHash;
    };
    const sdkClient = new PayGateClient({
        payFunction,
        payerAddress: account.address,
        autoSession: true,
        sessionDeposit: config.sessionDeposit,
    });
    const pricingCache = new PricingCacheManager(config.gatewayUrl);
    const spendTracker = new SpendTracker(config.spendLimitDaily, config.spendLimitMonthly);
    const sessionManager = new SessionManager(sdkClient, config);
    const activeTraces = new Map();
    await sessionManager.tryResumeSession();
    const TOOLS = [
        {
            name: 'paygate_discover',
            description: 'List available PayGate-protected APIs with pricing. Optionally provide a goal to rank APIs by relevance with usage examples.',
            inputSchema: {
                type: 'object',
                properties: {
                    goal: { type: 'string', description: 'Optional: describe what you want to accomplish. APIs will be ranked by relevance.' },
                },
            },
        },
        {
            name: 'paygate_call',
            description: 'Call any PayGate-protected API endpoint. Handles session creation, payment, and authentication automatically. Returns the upstream response plus payment proof (cost, explorer link, remaining balance).',
            inputSchema: {
                type: 'object',
                properties: {
                    method: { type: 'string', enum: ['GET', 'POST', 'PUT', 'DELETE'], description: 'HTTP method' },
                    path: { type: 'string', description: 'API path (e.g. /v1/search)' },
                    body: { type: 'object', description: 'Request body (for POST/PUT)' },
                    headers: { type: 'object', description: 'Additional request headers', additionalProperties: { type: 'string' } },
                },
                required: ['method', 'path'],
            },
        },
        {
            name: 'paygate_budget',
            description: 'Check current spending status: session balance, total spent today/this month, daily/monthly limits, and remaining budget. No payment required.',
            inputSchema: { type: 'object', properties: {} },
        },
        {
            name: 'paygate_estimate',
            description: 'Estimate the cost of a planned sequence of API calls. Returns total cost, per-endpoint breakdown, and whether the plan fits within your budget.',
            inputSchema: {
                type: 'object',
                properties: {
                    calls: {
                        type: 'array',
                        description: 'List of planned calls',
                        items: {
                            type: 'object',
                            properties: {
                                endpoint: { type: 'string', description: "Endpoint (e.g. 'POST /v1/search')" },
                                count: { type: 'number', description: 'Number of calls' },
                            },
                            required: ['endpoint', 'count'],
                        },
                    },
                },
                required: ['calls'],
            },
        },
        {
            name: 'paygate_trace',
            description: 'Track costs across a multi-step workflow. Start a named trace, make calls, then stop it to get total cost breakdown with explorer links.',
            inputSchema: {
                type: 'object',
                properties: {
                    action: { type: 'string', enum: ['start', 'stop'], description: "'start' begins a trace, 'stop' ends it and returns summary" },
                    name: { type: 'string', description: 'Unique name for this trace' },
                },
                required: ['action', 'name'],
            },
        },
    ];
    const discoverHandler = handleDiscover(pricingCache);
    const callHandler = handleCall(sdkClient, config, spendTracker, sessionManager, pricingCache, activeTraces);
    const budgetHandler = handleBudget(spendTracker, sessionManager, config);
    const estimateHandler = handleEstimate(pricingCache, spendTracker);
    const traceHandler = handleTrace(activeTraces);
    const server = new Server({ name: 'paygate', version: '0.5.0' }, { capabilities: { tools: {} } });
    server.setRequestHandler(ListToolsRequestSchema, async () => ({
        tools: TOOLS,
    }));
    server.setRequestHandler(CallToolRequestSchema, async (request) => {
        const { name, arguments: args } = request.params;
        switch (name) {
            case 'paygate_discover':
                return discoverHandler((args ?? {}));
            case 'paygate_call':
                return callHandler((args ?? {}));
            case 'paygate_budget':
                return budgetHandler();
            case 'paygate_estimate':
                return estimateHandler((args ?? {}));
            case 'paygate_trace':
                return traceHandler((args ?? {}));
            default:
                return errorToMcpContent(invalidInput(`Unknown tool: ${name}`));
        }
    });
    const shutdown = () => {
        process.stderr.write('[paygate] Shutting down...\n');
        sessionManager.logShutdownState();
        const record = spendTracker.getRecord();
        process.stderr.write(`[paygate] Total spent this session: ${formatUsd(record.totalSpentToday)} across ${record.callCount} calls\n`);
        process.exit(0);
    };
    process.on('SIGINT', shutdown);
    process.on('SIGTERM', shutdown);
    const transport = new StdioServerTransport();
    await server.connect(transport);
    process.stderr.write(`[paygate] MCP server started — gateway: ${config.gatewayUrl}, agent: ${config.agentName}, payer: ${config.payerAddress}\n`);
}
main().catch((err) => {
    process.stderr.write(`[paygate] Fatal: ${err instanceof Error ? err.message : String(err)}\n`);
    process.exit(1);
});
