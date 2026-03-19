import prompts from 'prompts';

export interface WizardAnswers {
  directory: string;
  description: string;
  price: string;
  walletAddress: string;
}

export function validatePrice(value: string): boolean | string {
  const num = parseFloat(value);
  if (isNaN(num) || num < 0) return 'Must be a non-negative number';
  const parts = value.split('.');
  if (parts.length === 2 && parts[1].length > 6) return 'Maximum 6 decimal places';
  return true;
}

export function validateAddress(value: string): boolean | string {
  if (!value.startsWith('0x')) return 'Must start with 0x';
  if (value.length !== 42) return 'Must be exactly 42 characters';
  if (!/^0x[0-9a-fA-F]{40}$/.test(value)) return 'Invalid hex characters';
  return true;
}

export function validateDirectory(value: string): boolean | string {
  if (!value || value.trim().length === 0) return 'Directory name is required';
  if (/[<>:"|?*]/.test(value)) return 'Invalid characters in directory name';
  return true;
}

export async function runWizard(targetArg?: string): Promise<WizardAnswers> {
  const questions: prompts.PromptObject[] = [];

  if (!targetArg) {
    questions.push({
      type: 'text',
      name: 'directory',
      message: 'Project directory name?',
      validate: (v: string) => validateDirectory(v),
    });
  }

  questions.push(
    {
      type: 'text',
      name: 'description',
      message: 'What does your API do?',
      validate: (v: string) => {
        if (!v || v.trim().length === 0) return 'Description is required';
        if (v.length > 200) return 'Maximum 200 characters';
        return true;
      },
    },
    {
      type: 'text',
      name: 'price',
      message: 'Price per request in USDC?',
      initial: '0.001',
      validate: (v: string) => validatePrice(v),
    },
    {
      type: 'text',
      name: 'walletAddress',
      message: 'Your Tempo wallet address?',
      hint: 'Generate one with: cast wallet new',
      validate: (v: string) => validateAddress(v),
    },
  );

  const response = await prompts(questions, {
    onCancel: () => {
      console.log();
      console.log('  Cancelled.');
      process.exit(1);
    },
  });

  return {
    directory: targetArg || response.directory,
    description: response.description,
    price: response.price,
    walletAddress: response.walletAddress,
  };
}
