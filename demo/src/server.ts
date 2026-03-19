import express from 'express';
import { errorHandler } from './lib/errors.js';
import pricingRouter from './routes/pricing.js';
import searchRouter from './routes/search.js';
import scrapeRouter, { initBrowser, closeBrowser } from './routes/scrape.js';
import imageRouter from './routes/image.js';
import summarizeRouter from './routes/summarize.js';

// Check required environment variables
const REQUIRED_ENV = [
  { key: 'BRAVE_API_KEY', hint: 'get a free API key at https://brave.com/search/api/' },
  { key: 'REPLICATE_API_TOKEN', hint: 'get a token at https://replicate.com/account/api-tokens' },
  { key: 'ANTHROPIC_API_KEY', hint: 'get a key at https://console.anthropic.com/' },
];

for (const { key, hint } of REQUIRED_ENV) {
  if (!process.env[key]) {
    console.error(`error: missing required environment variable: ${key}`);
    console.error(`  hint: ${hint}`);
    process.exit(1);
  }
}

const PORT = Number(process.env.PORT) || 3001;

const app = express();
app.use(express.json({ limit: '10mb' }));

// Routes
app.use(pricingRouter);
app.use(searchRouter);
app.use(scrapeRouter);
app.use(imageRouter);
app.use(summarizeRouter);

// Error handler (must be last)
app.use(errorHandler);

export { app };

// Start server (only when run directly, not when imported for tests)
const isDirectRun = process.argv[1]?.endsWith('server.ts') || process.argv[1]?.endsWith('server.js');
if (isDirectRun) {
  (async () => {
    await initBrowser();

    const server = app.listen(PORT, () => {
      console.log(`  PayGate Demo Server v0.1.0`);
      console.log(`  Listening on :${PORT}`);
      console.log(`  Endpoints: /v1/pricing, /v1/search, /v1/scrape, /v1/image, /v1/summarize`);
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
