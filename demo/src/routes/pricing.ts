import { Router } from 'express';

const router = Router();

router.get('/v1/pricing', (_req, res) => {
  res.json({
    apis: [
      {
        endpoint: 'POST /v1/search',
        price: '0.002',
        currency: 'USDC',
        description: 'Web search powered by Brave Search API',
        example_request: { query: 'latest AI news', count: 5 },
        example_response_fields: ['title', 'url', 'description'],
      },
      {
        endpoint: 'POST /v1/scrape',
        price: '0.001',
        currency: 'USDC',
        description: 'Web scraper — returns clean markdown from any URL',
        example_request: { url: 'https://example.com' },
        example_response_fields: ['title', 'content', 'markdown'],
      },
      {
        endpoint: 'POST /v1/image',
        price: '0.01',
        currency: 'USDC',
        description: 'Image generation via Replicate SDXL',
        example_request: { prompt: 'a cat astronaut, digital art' },
        example_response_fields: ['image_url', 'model', 'generation_time_ms'],
      },
      {
        endpoint: 'POST /v1/summarize',
        price: '0.003',
        currency: 'USDC',
        description: 'Text summarization via Claude Haiku',
        example_request: { text: 'Long article text...', max_length: 200 },
        example_response_fields: ['summary', 'model', 'input_tokens', 'output_tokens'],
      },
    ],
  });
});

export default router;
