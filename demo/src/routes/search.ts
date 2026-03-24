import { Router } from 'express';
import { upstreamFetch } from '../lib/upstream.js';
import { requireString, optionalNumber } from '../lib/validation.js';

const router = Router();

const BRAVE_API_KEY = process.env.BRAVE_API_KEY ?? '';

const MOCK_RESULTS_DB: Record<string, Array<{ title: string; url: string; description: string; age: string }>> = {
  _default: [
    {
      title: 'Understanding Modern Web APIs - Developer Guide',
      url: 'https://developer.example.com/guides/web-apis',
      description: 'A comprehensive guide to building and consuming modern web APIs, covering REST, GraphQL, and emerging patterns for 2025.',
      age: '3 days ago',
    },
    {
      title: 'The State of AI in 2025: Key Trends and Breakthroughs',
      url: 'https://techreview.example.com/ai-trends-2025',
      description: 'An overview of the most significant AI developments this year, from multimodal models to autonomous agents and their real-world applications.',
      age: '1 week ago',
    },
    {
      title: 'How to Build a Pay-Per-Call API Marketplace',
      url: 'https://blog.example.com/api-marketplace-tutorial',
      description: 'Step-by-step tutorial on creating an API marketplace with micropayments, authentication, and usage tracking.',
      age: '2 weeks ago',
    },
    {
      title: 'Brave Search API Documentation',
      url: 'https://api.search.brave.com/docs',
      description: 'Official documentation for the Brave Search API, including endpoints, authentication, rate limits, and response formats.',
      age: '1 month ago',
    },
    {
      title: 'Comparing Search APIs: Brave, Google, and Bing',
      url: 'https://devtools.example.com/search-api-comparison',
      description: 'A detailed comparison of major search APIs by cost, quality, speed, and developer experience.',
      age: '5 days ago',
    },
  ],
};

function getMockResults(query: string, count: number) {
  const results = MOCK_RESULTS_DB._default.slice(0, count).map(r => ({
    ...r,
    // Inject query into first result title for realism
    title: r.title,
    description: r.description,
  }));

  // Make first result feel query-specific
  if (results.length > 0) {
    results[0] = {
      ...results[0],
      title: `${query} - Top Results and Analysis`,
      description: `Comprehensive coverage of "${query}" including recent developments, expert opinions, and practical resources.`,
    };
  }

  return results;
}

router.post('/v1/search', async (req, res, next) => {
  try {
    const query = requireString(req.body?.query, 'query');
    const count = optionalNumber(req.body?.count, 'count', 5, 1, 20);

    // Mock mode when BRAVE_API_KEY is not set
    if (!BRAVE_API_KEY) {
      const results = getMockResults(query, count);
      const tokenCount = results.length * 50;
      res.setHeader('X-Token-Count', tokenCount.toString());
      console.log(`[search] query="${query}" mode=MOCK status=200`);
      res.json({
        results,
        query,
        result_count: results.length,
        _demo: true,
        _mock: true,
        _note: 'This is a demo response. In production, this endpoint returns real data from Brave Search.',
      });
      return;
    }

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

    const tokenCount = results.length * 50;
    res.setHeader('X-Token-Count', tokenCount.toString());
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
