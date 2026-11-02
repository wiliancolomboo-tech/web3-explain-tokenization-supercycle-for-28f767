import express, { Request, Response } from "express";
import { Connection } from "@solana/web3.js";

const app = express();
const port = process.env.PORT || 8080;
const connection = new Connection(process.env.SOLANA_RPC_URL || "https://api.devnet.solana.com", "confirmed");

app.use(express.json());

app.get("/health", async (req: Request, res: Response) => {
  try {
    const slot = await connection.getSlot();
    const version = await connection.getVersion();
    res.status(200).json({
      status: "UP",
      engine: "Aegis Institutional Credit Supercycle",
      currentSlot: slot,
      solanaCoreVersion: version["solana-core"],
      timestamp: new Date().toISOString(),
    });
  } catch (error: any) {
    res.status(503).json({
      status: "DEGRADED",
      error: error.message,
    });
  }
});

app.listen(port, () => {
  console.log(`[AEGIS TELEMETRY] Healthcheck listening on port ${port}`);
});