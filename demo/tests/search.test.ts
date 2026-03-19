import { describe, it, expect, vi, beforeEach } from 'vitest';
import request from 'supertest';
import { app } from '../src/server.js';

// Mock global fetch for upstream calls
const mockFetch = vi.fn();
vi.stubGlobal('fetch', mockFetch);

beforeEach(() => {
  mockFetch.mockReset();
});

describe('POST /v1/search', () => {
  it('returns search results on success', async () => {
    mockFetch.mockResolvedValueOnce(
      new Response(JSON.stringify({
        web: {
          results: [
            { title: 'AI News', url: 'https://example.com/ai', description: 'Latest AI developments', age: '2h' },
            { title: 'ML Update', url: 'https://example.com/ml', description: 'Machine learning news' },
          ],
        },
      }), { status: 200 }),
    );

    const res = await request(app)
      .post('/v1/search')
      .send({ query: 'AI news', count: 2 });

    expect(res.status).toBe(200);
    expect(res.body.results).toHaveLength(2);
    expect(res.body.query).toBe('AI news');
    expect(res.body.result_count).toBe(2);
    expect(res.body.results[0]).toHaveProperty('title', 'AI News');
    expect(res.body.results[0]).toHaveProperty('url');
    expect(res.body.results[0]).toHaveProperty('description');
  });

  it('returns 400 for missing query', async () => {
    const res = await request(app)
      .post('/v1/search')
      .send({});

    expect(res.status).toBe(400);
    expect(res.body.error).toBe('ValidationError');
  });

  it('returns 400 for count out of range', async () => {
    const res = await request(app)
      .post('/v1/search')
      .send({ query: 'test', count: 50 });

    expect(res.status).toBe(400);
  });

  it('returns 502 when upstream returns 500', async () => {
    mockFetch.mockResolvedValueOnce(
      new Response('Internal Server Error', { status: 500 }),
    );

    const res = await request(app)
      .post('/v1/search')
      .send({ query: 'test' });

    expect(res.status).toBe(502);
  });
});
