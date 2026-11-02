import http from "http";

const server = http.createServer((req, res) => {
  if (req.url === "/health" && req.method === "GET") {
    res.writeHead(200, { "Content-Type": "application/json" });
    res.end(
      JSON.stringify({
        status: "HEALTHY",
        uptimeSeconds: process.uptime(),
        timestamp: new Date().toISOString(),
        engine: "Solana RWA Supercycle Token-2022 Hook Daemon",
      })
    );
  } else {
    res.writeHead(404);
    res.end();
  }
});

server.listen(8080, () => {
  console.log("Telemetry daemon running on port 8080");
});