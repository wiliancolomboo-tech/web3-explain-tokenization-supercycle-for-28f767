// File: src/agent/rwa_credit_orchestrator.ts

import * as anchor from "@coral-xyz/anchor";
import { Program, web3 } from "@coral-xyz/anchor";
import axios from "axios";
import { Resend } from "resend";
import * as dotenv from "dotenv";
import * as crypto from "crypto";

dotenv.config();

export interface ProtocolConfig {
  resendApiKey: string;
  slackWebhookUrl: string;
  rpcUrl: string;
  solanaKeypairJson: string;
  programId: string;
  senderEmail: string;
}

export class RwaCreditOrchestrator {
  private connection: web3.Connection;
  private wallet: anchor.Wallet;
  private resend: Resend;
  private slackWebhookUrl: string;
  private senderEmail: string;

  constructor(config: ProtocolConfig) {
    this.connection = new web3.Connection(config.rpcUrl, "confirmed");
    const secretKey = Uint8Array.from(JSON.parse(config.solanaKeypairJson));
    const keypair = web3.Keypair.fromSecretKey(secretKey);
    this.wallet = new anchor.Wallet(keypair);
    this.resend = new Resend(config.resendApiKey);
    this.slackWebhookUrl = config.slackWebhookUrl;
    this.senderEmail = config.senderEmail;
  }

  /**
   * Deterministic SHA-256 for institutional off-chain KYC records
   */
  public generateKycHash(legalEntityName: string, taxId: string, jurisdiction: string): Buffer {
    const raw = `${legalEntityName}|${taxId}|${jurisdiction}|SALT_RWA_2025`;
    return crypto.createHash("sha256").update(raw).digest();
  }

  /**
   * Enterprise Alert Pipeline: Slack Webhook
   */
  public async postSlackAlert(message: string, fields: Record<string, string>): Promise<void> {
    try {
      const payload = {
        text: `*SolRWA Institutional Alert:* ${message}`,
        attachments: [
          {
            color: "#00FFA3",
            fields: Object.entries(fields).map(([k, v]) => ({
              title: k,
              value: v,
              short: true,
            })),
            ts: Math.floor(Date.now() / 1000),
          },
        ],
      };
      await axios.post(this.slackWebhookUrl, payload, {
        headers: { "Content-Type": "application/json" },
        timeout: 5000,
      });
      console.log(`[SLACK] Dispatched alert: ${message}`);
    } catch (err: any) {
      console.error(`[SLACK ERROR] Failed to send webhook: ${err.message}`);
    }
  }

  /**
   * Enterprise Alert Pipeline: Resend Transaction Reciept / Audit Statement
   */
  public async sendInvestorAuditStatement(
    recipientEmail: string,
    investorAddress: string,
    poolId: string,
    amount: string,
    tranche: string,
    txSignature: string
  ): Promise<void> {
    try {
      const htmlContent = `
        <div style="font-family: Arial, sans-serif; background-color: #0b0e14; color: #ffffff; padding: 24px;">
          <h2 style="color: #14F195;">SolRWA Protocol // Institutional Allocation Executed</h2>
          <p>This automated statement confirms settlement of your private credit allocation on Solana.</p>
          <hr style="border-color: #2a2e39;"/>
          <table style="width: 100%; text-align: left; margin-top: 16px;">
            <tr><td><strong>Investor Pubkey:</strong></td><td>${investorAddress}</td></tr>
            <tr><td><strong>Credit Facility Pool:</strong></td><td>${poolId}</td></tr>
            <tr><td><strong>Selected Tranche:</strong></td><td>${tranche}</td></tr>
            <tr><td><strong>Capital Committed:</strong></td><td>${amount} USDC</td></tr>
            <tr><td><strong>Solana Transaction:</strong></td><td><a href="https://solscan.io/tx/${txSignature}?cluster=custom" style="color: #9945FF;">${txSignature.slice(0, 16)}...</a></td></tr>
          </table>
          <p style="margin-top: 24px; font-size: 12px; color: #737887;">
            Regulated Token-2022 Transfer Hook security enforced. Retain this record for annual financial audits.
          </p>
        </div>
      `;

      await this.resend.emails.send({
        from: this.senderEmail,
        to: recipientEmail,
        subject: `[CONFIRMATION] Allocation Settled: Pool ${poolId} (${tranche})`,
        html: htmlContent,
      });
      console.log(`[RESEND] Audit email dispatched to ${recipientEmail}`);
    } catch (err: any) {
      console.error(`[RESEND ERROR] Failed to send email: ${err.message}`);
    }
  }

  /**
   * Continuous WebSocket Event Daemon
   */
  public async startTelemetryDaemon(program: Program): Promise<void> {
    console.log("[DAEMON] Initializing real-time on-chain event listeners...");

    program.addEventListener("RepaymentProcessed", async (event: any, slot: number) => {
      console.log(`[EVENT] Waterfall Repayment at Slot ${slot}:`, event);
      await this.postSlackAlert("Repayment Waterfall Executed", {
        "Pool Identifier": event.poolId,
        "Total Repayment": `${(event.totalRepayment.toNumber() / 1e6).toFixed(2)} USDC`,
        "Senior Tranche Distribution": `${(event.seniorPortion.toNumber() / 1e6).toFixed(2)} USDC`,
        "Junior Residual Yield": `${(event.juniorPortion.toNumber() / 1e6).toFixed(2)} USDC`,
      });
    });

    program.addEventListener("PoolFullyFunded", async (event: any, slot: number) => {
      console.log(`[EVENT] Credit Pool Funded at Slot ${slot}:`, event);
      await this.postSlackAlert("Pool Reached Target - Loan Active", {
        "Pool Identifier": event.poolId,
        "Start Timestamp": new Date(event.startTimestamp.toNumber() * 1000).toISOString(),
        "Maturity Date": new Date(event.maturityTimestamp.toNumber() * 1000).toISOString(),
      });
    });
  }
}

// CLI Execution Entry Point
async function main() {
  const config: ProtocolConfig = {
    resendApiKey: process.env.RESEND_API_KEY || "re_test_key_dummy_12345",
    slackWebhookUrl: process.env.SLACK_WEBHOOK_URL || "https://api.example.com/webhooks/alerts/placeholder",
    rpcUrl: process.env.SOLANA_RPC_URL || "http://127.0.0.1:8899",
    solanaKeypairJson: process.env.SOLANA_KEYPAIR_JSON || `[${Array(64).fill(1).join(",")}]`,
    programId: "Credit1111111111111111111111111111111111111",
    senderEmail: process.env.SENDER_EMAIL || "servicing@solrwa-prime.fi",
  };

  const orchestrator = new RwaCreditOrchestrator(config);
  console.log("[ORCHESTRATOR] SolRWA Enterprise Telemetry Worker Active.");
  
  // Healthcheck ping
  await orchestrator.postSlackAlert("SolRWA Core Telemetry Online", {
    "Target Environment": "Production Mainnet-Beta RPC Gateway",
    "Timestamp": new Date().toISOString(),
  });
}

if (require.main === module) {
  main().catch((err) => {
    console.error("Fatal exception in Orchestrator:", err);
    process.exit(1);
  });
}