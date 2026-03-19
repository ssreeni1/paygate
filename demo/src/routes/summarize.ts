import { Router } from 'express';
import { upstreamFetch } from '../lib/upstream.js';
import { requireStringMaxLength, optionalNumber } from '../lib/validation.js';

const router = Router();

const ANTHROPIC_API_KEY = process.env.ANTHROPIC_API_KEY ?? '';

function generateMockSummary(text: string, maxLength: number): string {
  // Extract first few sentences to create a realistic-looking summary
  const sentences = text.match(/[^.!?]+[.!?]+/g) ?? [];
  const firstFew = sentences.slice(0, 3).join(' ').trim();

  if (firstFew.length > 20) {
    // Build summary from actual input to feel realistic
    const words = firstFew.split(/\s+/).slice(0, maxLength);
    return `The text discusses ${words.slice(0, 5).join(' ').toLowerCase().replace(/[^a-z0-9\s]/g, '')}. ${words.length > 10 ? 'Key points include the main themes presented in the content, along with supporting details and relevant context.' : 'The content provides a brief overview of the topic.'} This summary captures the essential information from the provided text.`;
  }

  return 'The provided text covers several key topics. The main points relate to the subject matter discussed, with supporting details and context. The content presents a clear overview that can be referenced for the essential takeaways.';
}

router.post('/v1/summarize', async (req, res, next) => {
  try {
    const text = requireStringMaxLength(req.body?.text, 'text', 100_000);
    const maxLength = optionalNumber(req.body?.max_length, 'max_length', 200, 10, 2000);

    // Mock mode when ANTHROPIC_API_KEY is not set
    if (!ANTHROPIC_API_KEY) {
      const summary = generateMockSummary(text, maxLength);
      const inputTokens = Math.ceil(text.length / 4); // rough approximation
      const outputTokens = Math.ceil(summary.length / 4);

      console.log(`[summarize] input_chars=${text.length} mode=MOCK status=200`);
      res.json({
        summary,
        model: 'claude-3-5-haiku-20241022',
        input_tokens: inputTokens,
        output_tokens: outputTokens,
        _demo: true,
        _mock: true,
        _note: 'This is a demo response. In production, this endpoint returns real data from Claude Haiku.',
      });
      return;
    }

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
