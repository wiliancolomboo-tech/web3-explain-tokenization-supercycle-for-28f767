import http from 'http';
import { Connection } from '@solana/web3.js';
import * as dotenv from 'dotenv';
dotenv.config();

const port = process.env.PORT || 8080;
const rpcUrl = process.env.SOLANA_RPC_URL || 'https://api.devnet.solana.com';
const connection = new Connection(rpcUrl, 'confirmed');

const server = http.createServer(async (req, res) => {
  if (req.url === '/health' && req.method === 'GET') {
    try {
      const version = await connection.getVersion();
      const slot = await connection.getSlot();

      res.writeHead(200, { 'Content-Type': 'application/json' });
      res.end(
        JSON.stringify({
          status: 'HEALTHY',
          service: 'solana-rwa-credit-daemon',
          clusterVersion: version['solana-core'],
          currentSlot: slot,
          timestamp: new Date().toISOString(),
        })
      );
    } catch (err: any) {
      res.writeHead(503, { 'Content-Type': 'application/json' });
      res.end(
        JSON.stringify({
          status: 'UNHEALTHY',
          error: err.message,
          timestamp: new Date().toISOString(),
        })
      );
    }
  } else {
    res.writeHead(404, { 'Content-Type': 'application/json' });
    res.end(JSON.stringify({ message: 'Route not found' }));
  }
});

server.listen(port, () => {
  console.log(`[Enterprise Healthcheck Server] Listening on port ${port}`);
});