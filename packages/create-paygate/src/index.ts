#!/usr/bin/env node

import { runWizard } from './wizard.js';
import { scaffold } from './scaffold.js';
import path from 'path';
import fs from 'fs';

const VERSION = '0.1.0';

async function main() {
  console.log();
  console.log(`  create-paygate v${VERSION}`);
  console.log('  ---------------------');
  console.log();

  const targetArg = process.argv[2];

  if (targetArg === '--help' || targetArg === '-h') {
    console.log('  Usage: npx create-paygate [directory]');
    console.log();
    console.log('  Creates a new PayGate-wrapped API project.');
    process.exit(0);
  }

  const answers = await runWizard(targetArg);

  const targetDir = path.resolve(answers.directory);

  if (fs.existsSync(targetDir) && !process.argv.includes('--force')) {
    const entries = fs.readdirSync(targetDir);
    if (entries.length > 0) {
      console.error(`  error: directory "${answers.directory}" already exists and is not empty`);
      console.error('    hint: use --force to overwrite');
      process.exit(1);
    }
  }

  fs.mkdirSync(targetDir, { recursive: true });

  console.log();
  console.log(`  Creating ${answers.directory}/...`);

  const files = scaffold(answers);

  for (const [filename, content] of Object.entries(files)) {
    const filePath = path.join(targetDir, filename);
    fs.mkdirSync(path.dirname(filePath), { recursive: true });
    fs.writeFileSync(filePath, content);
    const label = filename.padEnd(20);
    const desc = fileDescriptions[filename] || '';
    console.log(`    ${label} ${desc}`);
  }

  console.log();
  console.log('  Done! Next steps:');
  console.log();
  console.log(`    cd ${answers.directory}`);
  console.log('    cp .env.example .env');
  console.log('    # Edit .env with your PAYGATE_PRIVATE_KEY');
  console.log('    npm install');
  console.log('    npm start');
  console.log();
  console.log('  Deploy to fly.io:');
  console.log('    fly launch');
  console.log('    fly secrets set PAYGATE_PRIVATE_KEY=<your-key>');
  console.log('    fly deploy');
  console.log();
}

const fileDescriptions: Record<string, string> = {
  'paygate.toml': 'config',
  'server.js': 'sample API server',
  'Dockerfile': 'ready for fly.io',
  'README.md': 'quickstart guide',
  '.env.example': 'environment template',
  'package.json': 'npm package',
};

main().catch((err) => {
  console.error('  error:', err.message);
  process.exit(1);
});
