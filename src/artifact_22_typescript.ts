import http from "http";
import { SolanaRwaClient } from "./services/solanaClient";
import { NotificationService } from "./services/notificationService";
import { CONFIG } from "./config";
import winston from "winston";

const logger = winston.createLogger({
  level: "info",
  format: winston.format.combine(winston.format.timestamp(), winston.format.simple()),
  transports: [new winston.transports.Console()],
});

async function main() {
  logger.info("Starting Solana Institutional RWA Orchestration Service...");
  const client = new SolanaRwaClient();
  const notifier = new NotificationService();

  logger.info(`Connected to Solana Cluster: ${CONFIG.SOLANA_RPC_URL}`);
  logger.info(`Orchestrator Authority: ${client.keypair.publicKey.toBase58()}`);

  // Subscribe to program log streams for real-time compliance alerting
  client.connection.onLogs(
    client.programId,
    async (logs, ctx) => {
      if (logs.err) {
        logger.warn("Transaction execution failed in RWA program", { err: logs.err });
        await notifier.sendAlert("Transaction Reverted in Transfer Hook", "A transfer hook check failed.", {
          signature: logs.signature,
          slot: ctx.slot,
        });
        return;
      }

      for (const log of logs.logs) {
        if (log.includes("Transfer hook approved")) {
          logger.info(`Transfer verified: ${log}`);
        } else if (log.includes("Yield serviced")) {
          await notifier.sendAlert("PPA Yield Serviced", log, {
            signature: logs.signature,
          });
        }
      }
    },
    "confirmed"
  );

  // Healthcheck HTTP Server for Kubernetes / Container liveness probes
  const server = http.createServer((req, res) => {
    if (req.url === "/health" || req.url === "/") {
      res.writeHead(200, { "Content-Type": "application/json" });
      res.end(JSON.stringify({ status: "healthy", cluster: CONFIG.SOLANA_RPC_URL, timestamp: Date.now() }));
    } else {
      res.writeHead(404);
      res.end();
    }
  });

  server.listen(CONFIG.PORT, () => {
    logger.info(`Telemetry and Healthcheck server listening on port ${CONFIG.PORT}`);
  });
}

main().catch((err) => {
  logger.error("Fatal startup error", { err });
  process.exit(1);
});