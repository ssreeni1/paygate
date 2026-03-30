import express from 'express';
import { errorHandler } from './lib/errors.js';
import pricingRouter from './routes/pricing.js';
import searchRouter from './routes/search.js';
import scrapeRouter, { initBrowser, closeBrowser, isBrowserAvailable } from './routes/scrape.js';
import imageRouter from './routes/image.js';
import summarizeRouter from './routes/summarize.js';

// Always use 3001 internally — PayGate gateway handles the public port
const PORT = Number(process.env.DEMO_PORT) || 3001;

const app = express();
app.use(express.json({ limit: '10mb' }));

// Routes
app.use(pricingRouter);
app.use(searchRouter);
app.use(scrapeRouter);
app.use(imageRouter);
app.use(summarizeRouter);

// RPC proxy — forwards JSON-RPC calls to Tempo for the gateway process
// The Rust gateway can't reach rpc.moderato.tempo.xyz from some hosts (Railway EU),
// but Node.js can. This proxy runs on localhost:3001/rpc and the gateway uses it.
const TEMPO_RPC = process.env.TEMPO_RPC_URL || 'https://rpc.moderato.tempo.xyz';
app.post('/rpc', async (req, res) => {
  try {
    const resp = await fetch(TEMPO_RPC, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(req.body),
    });
    const data = await resp.json();
    res.json(data);
  } catch (err: any) {
    res.status(502).json({ jsonrpc: '2.0', error: { code: -32000, message: `RPC proxy error: ${err.message}` }, id: req.body?.id ?? null });
  }
});

// npm registry proxy — same connectivity issue as RPC. The Rust gateway uses
// NPM_REGISTRY_PROXY=http://localhost:3001/npm to reach registry.npmjs.org.
app.get('/npm/:package', async (req, res) => {
  const pkg = req.params.package;
  try {
    const resp = await fetch(`https://registry.npmjs.org/${encodeURIComponent(pkg)}`, {
      headers: { 'Accept': 'application/json' },
    });
    const data = await resp.text();
    res.status(resp.status).set('Content-Type', resp.headers.get('content-type') || 'application/json').send(data);
  } catch (err: any) {
    res.status(502).json({ error: `npm proxy error: ${err.message}` });
  }
});
// Scoped packages: /npm/@scope/package
app.get('/npm/@:scope/:package', async (req, res) => {
  const pkg = `@${req.params.scope}/${req.params.package}`;
  try {
    const resp = await fetch(`https://registry.npmjs.org/${encodeURIComponent(pkg)}`, {
      headers: { 'Accept': 'application/json' },
    });
    const data = await resp.text();
    res.status(resp.status).set('Content-Type', resp.headers.get('content-type') || 'application/json').send(data);
  } catch (err: any) {
    res.status(502).json({ error: `npm proxy error: ${err.message}` });
  }
});

// Error handler (must be last)
app.use(errorHandler);

export { app };

// Start server (only when run directly, not when imported for tests)
const isDirectRun = process.argv[1]?.endsWith('server.ts') || process.argv[1]?.endsWith('server.js');
if (isDirectRun) {
  (async () => {
    await initBrowser();

    const server = app.listen(PORT, () => {
      const searchMode = process.env.BRAVE_API_KEY ? 'LIVE (Brave Search)' : 'MOCK (set BRAVE_API_KEY for live)';
      const scrapeMode = isBrowserAvailable() ? 'LIVE (Playwright)' : 'MOCK (install Playwright for live)';
      const imageMode = process.env.REPLICATE_API_TOKEN ? 'LIVE (Replicate SDXL)' : 'MOCK (set REPLICATE_API_TOKEN for live)';
      const summarizeMode = process.env.ANTHROPIC_API_KEY ? 'LIVE (Claude Haiku)' : 'MOCK (set ANTHROPIC_API_KEY for live)';

      console.log('');
      console.log('  PayGate Demo Server');
      console.log(`    Search:    ${searchMode}`);
      console.log(`    Scrape:    ${scrapeMode}`);
      console.log(`    Image:     ${imageMode}`);
      console.log(`    Summarize: ${summarizeMode}`);
      console.log('');
      console.log(`  Listening on :${PORT}`);
      console.log('  Endpoints: /v1/pricing, /v1/search, /v1/scrape, /v1/image, /v1/summarize');
    });

    const shutdown = async () => {
      console.log('\n  Shutting down...');
      await closeBrowser();
      server.close(() => process.exit(0));
      // Force exit after 10s
      setTimeout(() => process.exit(1), 10_000);
    };

    process.on('SIGTERM', shutdown);
    process.on('SIGINT', shutdown);
  })();
}
