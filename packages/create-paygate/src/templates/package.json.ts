import type { WizardAnswers } from '../wizard.js';

export function packageJson(answers: WizardAnswers): string {
  return JSON.stringify(
    {
      name: answers.directory,
      version: '1.0.0',
      description: answers.description,
      main: 'server.js',
      scripts: {
        start: 'node server.js',
      },
      dependencies: {
        express: '^4.18',
      },
    },
    null,
    2,
  ) + '\n';
}
