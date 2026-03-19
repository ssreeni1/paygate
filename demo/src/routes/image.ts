import { Router } from 'express';
import { upstreamFetch } from '../lib/upstream.js';
import { requireStringMaxLength, optionalNumber } from '../lib/validation.js';
import { UpstreamTimeoutError } from '../lib/errors.js';

const router = Router();

const REPLICATE_API_TOKEN = process.env.REPLICATE_API_TOKEN ?? '';
const SDXL_VERSION = 'stability-ai/sdxl:7762fd07cf82c948538e41f63f77d685e02b063e37e496e96eefd46c929f9bdc';

router.post('/v1/image', async (req, res, next) => {
  try {
    const prompt = requireStringMaxLength(req.body?.prompt, 'prompt', 500);
    const width = optionalNumber(req.body?.width, 'width', 1024, 256, 2048);
    const height = optionalNumber(req.body?.height, 'height', 1024, 256, 2048);

    const start = Date.now();

    // Create prediction
    const createResp = await upstreamFetch(
      'https://api.replicate.com/v1/predictions',
      {
        method: 'POST',
        headers: {
          'Authorization': `Bearer ${REPLICATE_API_TOKEN}`,
          'Content-Type': 'application/json',
        },
        body: JSON.stringify({
          version: SDXL_VERSION.split(':')[1],
          input: { prompt, width, height },
        }),
      },
      { timeout: 15_000 },
    );

    const prediction = await createResp.json() as {
      id: string;
      status: string;
      urls?: { get: string };
    };

    if (!prediction.urls?.get) {
      throw new Error('Replicate did not return prediction URL');
    }

    // Poll for completion (max 60s, every 2s)
    const pollUrl = prediction.urls.get;
    const deadline = Date.now() + 60_000;
    let result: { status: string; output?: string[]; error?: string } = prediction;

    while (result.status !== 'succeeded' && result.status !== 'failed') {
      if (Date.now() > deadline) {
        throw new UpstreamTimeoutError('replicate polling');
      }
      await new Promise(r => setTimeout(r, 2000));

      const pollResp = await upstreamFetch(
        pollUrl,
        {
          method: 'GET',
          headers: { 'Authorization': `Bearer ${REPLICATE_API_TOKEN}` },
        },
        { timeout: 10_000 },
      );
      result = await pollResp.json() as typeof result;
    }

    if (result.status === 'failed') {
      throw new Error(`Image generation failed: ${result.error ?? 'unknown error'}`);
    }

    const imageUrl = result.output?.[0] ?? '';
    const elapsed = Date.now() - start;
    console.log(`[image] prompt="${prompt.slice(0, 40)}..." upstream_ms=${elapsed} status=200`);

    res.json({
      image_url: imageUrl,
      prompt,
      model: 'sdxl',
      generation_time_ms: elapsed,
    });
  } catch (err) {
    next(err);
  }
});

export default router;
