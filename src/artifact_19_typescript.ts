// Embedded in client/orchestrator.ts HTTP daemon
import * as http from "http";

const server = http.createServer((req, res) => {
  if (req.url === "/health") {
    res.writeHead(200, { "Content-Type": "application/json" });
    res.end(
      JSON.stringify({
        status: "UP",
        cluster: "Solana Devnet/Mainnet",
        component: "Token-2022 Transfer Hook Watcher",
        uptimeSeconds: process.uptime(),
        timestamp: new Date().toISOString(),
      })
    );
  } else {
    res.writeHead(404);
    res.end();
  }
});

server.listen(process.env.PORT || 8080, () => {
  console.log(`[HEALTHCHECK] Telemetry endpoint running on port ${process.env.PORT || 8080}`);
});