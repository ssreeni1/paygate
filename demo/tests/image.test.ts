import { describe, it, expect, vi, beforeEach } from 'vitest';
import request from 'supertest';
import { app } from '../src/server.js';

const mockFetch = vi.fn();
vi.stubGlobal('fetch', mockFetch);

beforeEach(() => {
  mockFetch.mockReset();
});

describe('POST /v1/image', () => {
  it('returns image URL on success', async () => {
    // First call: create prediction
    mockFetch.mockResolvedValueOnce(
      new Response(JSON.stringify({
        id: 'pred_123',
        status: 'starting',
        urls: { get: 'https://api.replicate.com/v1/predictions/pred_123' },
      }), { status: 201 }),
    );

    // Second call: poll — succeeded
    mockFetch.mockResolvedValueOnce(
      new Response(JSON.stringify({
        status: 'succeeded',
        output: ['https://replicate.delivery/image.png'],
      }), { status: 200 }),
    );

    const res = await request(app)
      .post('/v1/image')
      .send({ prompt: 'a cat astronaut' });

    expect(res.status).toBe(200);
    expect(res.body.image_url).toBe('https://replicate.delivery/image.png');
    expect(res.body.model).toBe('sdxl');
    expect(res.body.prompt).toBe('a cat astronaut');
    expect(res.body.generation_time_ms).toBeGreaterThan(0);
  });

  it('returns 400 for missing prompt', async () => {
    const res = await request(app)
      .post('/v1/image')
      .send({});

    expect(res.status).toBe(400);
    expect(res.body.error).toBe('ValidationError');
  });

  it('returns 400 for prompt too long', async () => {
    const res = await request(app)
      .post('/v1/image')
      .send({ prompt: 'a'.repeat(501) });

    expect(res.status).toBe(400);
  });

  it('returns 502 when upstream returns 500', async () => {
    mockFetch.mockResolvedValueOnce(
      new Response('Server Error', { status: 500 }),
    );

    const res = await request(app)
      .post('/v1/image')
      .send({ prompt: 'test' });

    expect(res.status).toBe(502);
  });
});
