import type { WizardAnswers } from '../wizard.js';

export function serverJs(answers: WizardAnswers): string {
  return `const express = require('express');
const app = express();
app.use(express.json());

// Free pricing endpoint
app.get('/v1/pricing', (req, res) => {
  res.json({
    apis: [{
      endpoint: 'POST /v1/echo',
      price: '${answers.price}',
      currency: 'USDC',
      description: '${answers.description.replace(/'/g, "\\'")}'
    }]
  });
});

// Your API endpoint
app.post('/v1/echo', (req, res) => {
  // TODO: Replace with your actual API logic
  res.json({
    message: 'Hello from ${answers.directory}!',
    input: req.body,
    timestamp: new Date().toISOString()
  });
});

const PORT = process.env.PORT || 3000;
app.listen(PORT, () => {
  console.log('API server listening on port ' + PORT);
});
`;
}
