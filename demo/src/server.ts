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
