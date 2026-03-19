import type { WizardAnswers } from './wizard.js';
import { paygateToml } from './templates/paygate.toml.js';
import { serverJs } from './templates/server.js.js';
import { dockerfile } from './templates/dockerfile.js';
import { readmeMd } from './templates/readme.md.js';
import { envExample } from './templates/env.example.js';
import { packageJson } from './templates/package.json.js';

export function scaffold(answers: WizardAnswers): Record<string, string> {
  return {
    'paygate.toml': paygateToml(answers),
    'server.js': serverJs(answers),
    'Dockerfile': dockerfile(),
    'README.md': readmeMd(answers),
    '.env.example': envExample(),
    'package.json': packageJson(answers),
  };
}
