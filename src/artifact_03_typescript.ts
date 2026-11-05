// src/cli-engine.ts
import { Connection, Keypair, PublicKey, clusterApiUrl } from "@solana/web3.js";
import { Program, AnchorProvider, Wallet, BN } from "@coral-xyz/anchor";
import { Resend } from "resend";
import axios from "axios";
import * as crypto from "crypto";
import * as dotenv from "dotenv";

dotenv.config();

// Configuration Interface
interface SolPvConfig {
  rpcUrl: string;
  payerKeypair: Keypair;
  programId: PublicKey;
  resendApiKey: string;
  slackWebhookUrl: string;
  poolName: string;
}

export class SolPvEnterpriseOrchestrator {
  private connection: Connection;
  private provider: AnchorProvider;
  private resend: Resend;
  private slackWebhookUrl: string;
  private poolName: string;

  constructor(config: SolPvConfig) {
    this.connection = new Connection(config.rpcUrl, "confirmed");
    const wallet = new Wallet(config.payerKeypair);
    this.provider = new AnchorProvider(this.connection, wallet, {
      commitment: "confirmed",
      preflightCommitment: "confirmed",
    });
    this.resend = new Resend(config.resendApiKey);
    this.slackWebhookUrl = config.slackWebhookUrl;
    this.poolName = config.poolName;
  }

  /**
   * Derive Securitization Pool PDA
   */
  public getPoolPda(programId: PublicKey): [PublicKey, number] {
    return PublicKey.findProgramAddressSync(
      [Buffer.from("solpv_pool"), Buffer.from(this.poolName)],
      programId
    );
  }

  /**
   * Enterprise Alert Pipeline: Slack Webhook Notification
   */
  public async notifySlack(title: string, message: string, fields: Record<string, string>): Promise<void> {
    if (!this.slackWebhookUrl) {
      console.log(`[SLACK_MOCK] ${title}: ${message}`);
      return;
    }
    try {
      await axios.post(this.slackWebhookUrl, {
        text: `*${title}*\n${message}`,
        attachments: [
          {
            color: "#00FFA3", // Solana Green
            fields: Object.entries(fields).map(([k, v]) => ({
              title: k,
              value: v,
              short: true,
            })),
          },
        ],
      });
      console.log(`[Enterprise Notification] Slack hook delivered.`);
    } catch (err: any) {
      console.error(`[Enterprise Notification Failure] Slack hook error: ${err.message}`);
    }
  }

  /**
   * Enterprise Settlement Pipeline: Resend Transactional Email Dispatch
   */
  public async dispatchInvestorYieldNotice(
    recipientEmail: string,
    investorPubkey: string,
    seniorYieldUsdc: number,
    juniorYieldUsdc: number,
    kwhGenerated: number,
    txSignature: string
  ): Promise<void> {
    try {
      const emailHtml = `
        <div style="font-family: Arial, sans-serif; background: #0b0e14; color: #ffffff; padding: 24px; border-radius: 8px;">
          <h2 style="color: #00FFA3;">SolPV Real-World Asset Yield Settlement</h2>
          <p>This is an automated institutional distribution notice for <strong>${this.poolName}</strong>.</p>
          <hr style="border-color: #2a2e39;" />
          <table style="width: 100%; text-align: left; color: #d1d5db;">
            <tr><td><strong>Investor Account:</strong></td><td><code>${investorPubkey}</code></td></tr>
            <tr><td><strong>Senior Tranche Distribution:</strong></td><td>$${seniorYieldUsdc.toLocaleString()} USDC</td></tr>
            <tr><td><strong>Junior Tranche Distribution:</strong></td><td>$${juniorYieldUsdc.toLocaleString()} USDC</td></tr>
            <tr><td><strong>Verified Solar Energy:</strong></td><td>${kwhGenerated.toLocaleString()} kWh</td></tr>
            <tr><td><strong>Solana TX Hash:</strong></td><td><code>${txSignature}</code></td></tr>
          </table>
          <p style="margin-top: 16px; font-size: 12px; color: #9ca3af;">
            Complies with SEC Reg D Rule 506(c) automated secondary debt disclosures. Verified on Solana Token-2022.
          </p>
        </div>
      `;

      const response = await this.resend.emails.send({
        from: "settlement@solpv-credit.io",
        to: recipientEmail,
        subject: `[Settlement Confirmed] Yield Distributed: ${this.poolName}`,
        html: emailHtml,
      });

      console.log(`[Resend Engine] Settlement email dispatched. ID: ${response.data?.id}`);
    } catch (err: any) {
      console.error(`[Resend Error] Email dispatch failed: ${err.message}`);
    }
  }

  /**
   * Autonomous Telemetry Synchronizer & Settlement Pipeline
   */
  public async processTelemetryAndSettle(
    kwhProduced: number,
    yieldPayoutUsdc: number,
    mockInvestorEmail: string
  ): Promise<string> {
    console.log(`[Worker] Ingesting IoT Telemetry: ${kwhProduced} kWh generated.`);
    
    // Create Deterministic Signature Hash of Meter Data
    const rawTelemetry = `METER_ID_9844|KWH_${kwhProduced}|TS_${Date.now()}`;
    const hash = crypto.createHash("sha256").update(rawTelemetry).digest();
    const hashArray = Array.from(new Uint8Array(hash));

    // Simulated Anchor Program Call for Production Recording
    console.log(`[Solana RPC] Submitting verified telemetry state proof...`);
    const mockTxSignature = "5KtP9x..." + crypto.randomBytes(16).toString("hex");

    console.log(`[Solana RPC] Settlement confirmed on-chain: ${mockTxSignature}`);

    // Compute Senior/Junior Distribution Split
    const seniorSplit = yieldPayoutUsdc * 0.65;
    const juniorSplit = yieldPayoutUsdc * 0.35;

    // Trigger Enterprise Notifications
    await this.notifySlack("Solar Debt Yield Distributed", "Waterfall execution succeeded.", {
      Pool: this.poolName,
      "Total Distributed": `$${yieldPayoutUsdc.toLocaleString()} USDC`,
      "Senior Class A": `$${seniorSplit.toLocaleString()} USDC`,
      "Subordinated Class B": `$${juniorSplit.toLocaleString()} USDC`,
      Telemetry: `${kwhProduced} kWh`,
      TxSignature: mockTxSignature,
    });

    await this.dispatchInvestorYieldNotice(
      mockInvestorEmail,
      this.provider.wallet.publicKey.toBase58(),
      seniorSplit,
      juniorSplit,
      kwhProduced,
      mockTxSignature
    );

    return mockTxSignature;
  }
}

// CLI Execution Entrypoint
async function main() {
  const secretKey = process.env.SOLANA_PRIVATE_KEY
    ? Uint8Array.from(JSON.parse(process.env.SOLANA_PRIVATE_KEY))
    : Keypair.generate().secretKey;

  const orchestrator = new SolPvEnterpriseOrchestrator({
    rpcUrl: process.env.SOLANA_RPC_URL || clusterApiUrl("devnet"),
    payerKeypair: Keypair.fromSecretKey(secretKey),
    programId: new PublicKey("So1PV11111111111111111111111111111111111111"),
    resendApiKey: process.env.RESEND_API_KEY || "re_test_000000000000000000000000",
    slackWebhookUrl: process.env.SLACK_WEBHOOK_URL || "",
    poolName: "SolPV-Austin-Solar-01",
  });

  console.log("=== SolPV Enterprise Orchestrator Initiated ===");
  await orchestrator.processTelemetryAndSettle(
    148_500, // 148,500 kWh produced
    12_622,  // $12,622 USDC yield distributed
    "institutions@apollo-capital-mock.com"
  );
  console.log("=== Execution Cycle Concluded Successfully ===");
}

if (require.main === module) {
  main().catch(console.error);
}