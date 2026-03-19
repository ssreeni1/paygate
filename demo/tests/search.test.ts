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
  it('returns mock search results when BRAVE_API_KEY is not set', async () => {
    const res = await request(app)
      .post('/v1/search')
      .send({ query: 'AI news', count: 2 });

    expect(res.status).toBe(200);
    expect(res.body.results).toHaveLength(2);
    expect(res.body.query).toBe('AI news');
    expect(res.body.result_count).toBe(2);
    expect(res.body.results[0]).toHaveProperty('title');
    expect(res.body.results[0]).toHaveProperty('url');
    expect(res.body.results[0]).toHaveProperty('description');
    expect(res.body._demo).toBe(true);
    expect(res.body._mock).toBe(true);
    expect(res.body._note).toContain('Brave Search');
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
});
