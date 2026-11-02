import http from "http";

const server = http.createServer((req, res) => {
  if (req.url === "/health" && req.method === "GET") {
    res.writeHead(200, { "Content-Type": "application/json" });
    res.end(JSON.stringify({
      status: "UP",
      uptime: process.uptime(),
      timestamp: Date.now(),
      protocol: "Solana Structured Credit Protocol (SSCP)",
      tokenExtensions: ["TransferHook", "PermanentDelegate", "InterestBearing"]
    }));
  } else {
    res.writeHead(404);
    res.end();
  }
});

server.listen(8080, () => {
  console.log("Telemetry healthcheck server listening on port 8080");
});