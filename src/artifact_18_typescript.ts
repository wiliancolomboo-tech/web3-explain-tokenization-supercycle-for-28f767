import dotenv from "dotenv";
dotenv.config();

export const CONFIG = {
  SOLANA_RPC_URL: process.env.SOLANA_RPC_URL || "https://api.devnet.solana.com",
  SOLANA_WS_URL: process.env.SOLANA_WS_URL || "wss://api.devnet.solana.com",
  PROGRAM_ID: process.env.PROGRAM_ID || "RWA1111111111111111111111111111111111111111",
  AUTHORITY_PRIVATE_KEY: process.env.AUTHORITY_PRIVATE_KEY || "",
  RESEND_API_KEY: process.env.RESEND_API_KEY || "",
  RESEND_FROM_EMAIL: process.env.RESEND_FROM_EMAIL || "notifications@institutional-rwa.finance",
  ALERT_RECIPIENT_EMAIL: process.env.ALERT_RECIPIENT_EMAIL || "ops-audit@institutional-fund.com",
  SLACK_WEBHOOK_URL: process.env.SLACK_WEBHOOK_URL || "",
  PORT: parseInt(process.env.PORT || "8080", 10),
};