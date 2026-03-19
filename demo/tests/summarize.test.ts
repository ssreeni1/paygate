import { describe, it, expect, vi, beforeEach } from 'vitest';
import request from 'supertest';
import { app } from '../src/server.js';

const mockFetch = vi.fn();
vi.stubGlobal('fetch', mockFetch);

beforeEach(() => {
  mockFetch.mockReset();
});

describe('POST /v1/summarize', () => {
  it('returns summary on success', async () => {
    mockFetch.mockResolvedValueOnce(
      new Response(JSON.stringify({
        content: [{ type: 'text', text: 'This is a summary of the article.' }],
        model: 'claude-3-5-haiku-20241022',
        usage: { input_tokens: 150, output_tokens: 20 },
      }), { status: 200 }),
    );

    const res = await request(app)
      .post('/v1/summarize')
      .send({ text: 'A long article about artificial intelligence and its impact on society...', max_length: 50 });

    expect(res.status).toBe(200);
    expect(res.body.summary).toBe('This is a summary of the article.');
    expect(res.body.model).toBe('claude-3-5-haiku-20241022');
    expect(res.body.input_tokens).toBe(150);
    expect(res.body.output_tokens).toBe(20);
  });

  it('returns 400 for missing text', async () => {
    const res = await request(app)
      .post('/v1/summarize')
      .send({});

    expect(res.status).toBe(400);
    expect(res.body.error).toBe('ValidationError');
  });

  it('returns 400 for text too long', async () => {
    const res = await request(app)
      .post('/v1/summarize')
      .send({ text: 'x'.repeat(100_001) });

    expect(res.status).toBe(400);
  });

  it('returns 502 when upstream returns 500', async () => {
    mockFetch.mockResolvedValueOnce(
      new Response('Server Error', { status: 500 }),
    );

    const res = await request(app)
      .post('/v1/summarize')
      .send({ text: 'Some text to summarize' });

    expect(res.status).toBe(502);
  });
});
