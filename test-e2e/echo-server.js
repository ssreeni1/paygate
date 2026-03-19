// Simple echo server for e2e testing
const http = require('http');

const server = http.createServer((req, res) => {
  let body = '';
  req.on('data', chunk => body += chunk);
  req.on('end', () => {
    res.writeHead(200, { 'Content-Type': 'application/json' });
    res.end(JSON.stringify({
      echo: true,
      method: req.method,
      url: req.url,
      headers: req.headers,
      body: body || null,
    }));
  });
});

const port = process.env.ECHO_PORT || 9999;
server.listen(port, () => {
  console.log(`Echo server listening on :${port}`);
});
