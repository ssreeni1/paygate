import { Router } from 'express';
import { upstreamFetch } from '../lib/upstream.js';
import { requireStringMaxLength, optionalNumber } from '../lib/validation.js';

const router = Router();

const ANTHROPIC_API_KEY = process.env.ANTHROPIC_API_KEY ?? '';

router.post('/v1/summarize', async (req, res, next) => {
  try {
    const text = requireStringMaxLength(req.body?.text, 'text', 100_000);
    const maxLength = optionalNumber(req.body?.max_length, 'max_length', 200, 10, 2000);

    const start = Date.now();

    const response = await upstreamFetch(
      'https://api.anthropic.com/v1/messages',
      {
        method: 'POST',
        headers: {
          'x-api-key': ANTHROPIC_API_KEY,
          'anthropic-version': '2023-06-01',
          'Content-Type': 'application/json',
        },
        body: JSON.stringify({
          model: 'claude-3-5-haiku-20241022',
          max_tokens: 1024,
          system: `You are a summarization assistant. Summarize the following text in at most ${maxLength} words. Be concise and capture the key points.`,
          messages: [{ role: 'user', content: text }],
        }),
      },
      { timeout: 30_000 },
    );

    const data = await response.json() as {
      content: Array<{ type: string; text: string }>;
      model: string;
      usage: { input_tokens: number; output_tokens: number };
    };

    const summary = data.content
      .filter(c => c.type === 'text')
      .map(c => c.text)
      .join('\n');

    const elapsed = Date.now() - start;
    console.log(`[summarize] input_chars=${text.length} upstream_ms=${elapsed} status=200`);

    res.json({
      summary,
      model: data.model,
      input_tokens: data.usage.input_tokens,
      output_tokens: data.usage.output_tokens,
    });
  } catch (err) {
    next(err);
  }
});

export default router;
