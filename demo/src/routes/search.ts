import { Router } from 'express';
import { upstreamFetch } from '../lib/upstream.js';
import { requireString, optionalNumber } from '../lib/validation.js';

const router = Router();

const BRAVE_API_KEY = process.env.BRAVE_API_KEY ?? '';

router.post('/v1/search', async (req, res, next) => {
  try {
    const query = requireString(req.body?.query, 'query');
    const count = optionalNumber(req.body?.count, 'count', 5, 1, 20);

    const start = Date.now();
    const params = new URLSearchParams({ q: query, count: String(count) });
    const response = await upstreamFetch(
      `https://api.search.brave.com/res/v1/web/search?${params}`,
      {
        method: 'GET',
        headers: {
          'Accept': 'application/json',
          'X-Subscription-Token': BRAVE_API_KEY,
        },
      },
    );

    const data = await response.json() as {
      web?: { results?: Array<{ title: string; url: string; description: string; age?: string }> };
    };

    const results = (data.web?.results ?? []).map(r => ({
      title: r.title,
      url: r.url,
      description: r.description,
      age: r.age ?? null,
    }));

    const elapsed = Date.now() - start;
    console.log(`[search] query="${query}" upstream_ms=${elapsed} status=200`);

    res.json({
      results,
      query,
      result_count: results.length,
    });
  } catch (err) {
    next(err);
  }
});

export default router;
