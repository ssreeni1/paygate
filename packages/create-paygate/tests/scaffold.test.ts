import { describe, it, expect } from 'vitest';
import { scaffold } from '../src/scaffold.js';
import type { WizardAnswers } from '../src/wizard.js';

const testAnswers: WizardAnswers = {
  directory: 'my-test-api',
  description: 'Image classification API',
  price: '0.005',
  walletAddress: '0x7F3a000000000000000000000000000000000001',
};

describe('scaffold', () => {
  const files = scaffold(testAnswers);

  it('generates all 6 files', () => {
    expect(Object.keys(files)).toHaveLength(6);
    expect(files).toHaveProperty('paygate.toml');
    expect(files).toHaveProperty('server.js');
    expect(files).toHaveProperty('Dockerfile');
    expect(files).toHaveProperty('README.md');
    expect(files).toHaveProperty('.env.example');
    expect(files).toHaveProperty('package.json');
  });

  it('paygate.toml contains wallet address and price', () => {
    const toml = files['paygate.toml'];
    expect(toml).toContain('0x7F3a000000000000000000000000000000000001');
    expect(toml).toContain('0.005');
    expect(toml).toContain('my-test-api');
  });

  it('paygate.toml has valid TOML structure', () => {
    const toml = files['paygate.toml'];
    expect(toml).toContain('[gateway]');
    expect(toml).toContain('[tempo]');
    expect(toml).toContain('[provider]');
    expect(toml).toContain('[pricing]');
    expect(toml).toContain('[pricing.endpoints]');
  });

  it('server.js contains key Express patterns', () => {
    const js = files['server.js'];
    expect(js).toContain("require('express')");
    expect(js).toContain('/v1/pricing');
    expect(js).toContain('/v1/echo');
    expect(js).toContain('0.005');
    expect(js).toContain('Image classification API');
  });

  it('Dockerfile contains EXPOSE 8080', () => {
    expect(files['Dockerfile']).toContain('EXPOSE 8080');
  });

  it('Dockerfile contains paygate serve', () => {
    expect(files['Dockerfile']).toContain('paygate serve');
  });

  it('.env.example contains PAYGATE_PRIVATE_KEY', () => {
    expect(files['.env.example']).toContain('PAYGATE_PRIVATE_KEY');
  });

  it('README contains project name and description', () => {
    const readme = files['README.md'];
    expect(readme).toContain('# my-test-api');
    expect(readme).toContain('Image classification API');
  });

  it('package.json is valid JSON with correct name', () => {
    const pkg = JSON.parse(files['package.json']);
    expect(pkg.name).toBe('my-test-api');
    expect(pkg.description).toBe('Image classification API');
    expect(pkg.dependencies.express).toBeDefined();
  });
});
