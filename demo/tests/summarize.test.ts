import { describe, it, expect, vi, beforeEach } from 'vitest';
import request from 'supertest';
import { app } from '../src/server.js';

const mockFetch = vi.fn();
vi.stubGlobal('fetch', mockFetch);

beforeEach(() => {
  mockFetch.mockReset();
});

describe('POST /v1/summarize', () => {
  it('returns mock summary when ANTHROPIC_API_KEY is not set', async () => {
    const res = await request(app)
      .post('/v1/summarize')
      .send({ text: 'A long article about artificial intelligence and its impact on society.', max_length: 50 });

    expect(res.status).toBe(200);
    expect(res.body.summary).toBeTruthy();
    expect(res.body.model).toBe('claude-3-5-haiku-20241022');
    expect(res.body.input_tokens).toBeGreaterThan(0);
    expect(res.body.output_tokens).toBeGreaterThan(0);
    expect(res.body._demo).toBe(true);
    expect(res.body._mock).toBe(true);
    expect(res.body._note).toContain('Claude Haiku');
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
});
