import { Router } from 'express';
import { requireUrl } from '../lib/validation.js';
import { Semaphore } from '../lib/semaphore.js';
import type { Browser } from 'playwright';

const router = Router();

let browser: Browser | null = null;
const maxConcurrency = Number(process.env.PLAYWRIGHT_MAX_CONCURRENCY) || 3;
const semaphore = new Semaphore(maxConcurrency);

export async function initBrowser(): Promise<void> {
  const { chromium } = await import('playwright');
  browser = await chromium.launch({ headless: true });
  console.log(`[scrape] Playwright browser launched (max concurrency: ${maxConcurrency})`);
}

export async function closeBrowser(): Promise<void> {
  if (browser) {
    await browser.close();
    browser = null;
    console.log('[scrape] Playwright browser closed');
  }
}

router.post('/v1/scrape', async (req, res, next) => {
  try {
    const url = requireUrl(req.body?.url, 'url');

    if (!browser) {
      throw new Error('Playwright browser not initialized');
    }

    await semaphore.acquire();
    const start = Date.now();
    let context;

    try {
      context = await browser.newContext();
      const page = await context.newPage();

      await page.goto(url, { timeout: 15_000, waitUntil: 'networkidle' });

      const html = await page.content();

      const { JSDOM } = await import('jsdom');
      const { Readability } = await import('@mozilla/readability');

      const dom = new JSDOM(html, { url });
      const article = new Readability(dom.window.document).parse();

      let markdown = '';
      let title = '';
      if (article) {
        title = article.title;
        const TurndownService = (await import('turndown')).default;
        const td = new TurndownService();
        markdown = td.turndown(article.content);
      }

      const elapsed = Date.now() - start;
      console.log(`[scrape] url="${url}" upstream_ms=${elapsed} status=200`);

      res.json({
        title,
        content: markdown,
        url,
        scraped_at: new Date().toISOString(),
      });
    } finally {
      if (context) await context.close();
      semaphore.release();
    }
  } catch (err) {
    next(err);
  }
});

export default router;
