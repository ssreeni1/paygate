import { describe, it, expect, vi, beforeEach } from 'vitest';
import request from 'supertest';
import { app } from '../src/server.js';

const mockFetch = vi.fn();
vi.stubGlobal('fetch', mockFetch);

beforeEach(() => {
  mockFetch.mockReset();
});

describe('POST /v1/image', () => {
  it('returns mock image URL when REPLICATE_API_TOKEN is not set', async () => {
    const res = await request(app)
      .post('/v1/image')
      .send({ prompt: 'a cat astronaut' });

    expect(res.status).toBe(200);
    expect(res.body.image_url).toContain('placehold.co');
    expect(res.body.model).toBe('sdxl');
    expect(res.body.prompt).toBe('a cat astronaut');
    expect(res.body._demo).toBe(true);
    expect(res.body._mock).toBe(true);
    expect(res.body._note).toContain('Replicate');
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
});
