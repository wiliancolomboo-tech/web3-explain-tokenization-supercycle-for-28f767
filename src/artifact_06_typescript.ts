import { Connection, Keypair, PublicKey, clusterApiUrl } from "@solana/web3.js";
import { Program, AnchorProvider, Wallet } from "@coral-xyz/anchor";
import { Resend } from "resend";
import axios from "axios";
import * as dotenv from "dotenv";

dotenv.config();

// Enterprise Configuration Environment
const SOLANA_RPC_URL = process.env.SOLANA_RPC_URL || clusterApiUrl("devnet");
const RESEND_API_KEY = process.env.RESEND_API_KEY || "re_test_000000000000000000000000";
const DISCORD_WEBHOOK_URL = process.env.DISCORD_WEBHOOK_URL || "";
const PROGRAM_ID = new PublicKey(
  process.env.PROGRAM_ID || "RWA1111111111111111111111111111111111111111"
);

const resend = new Resend(RESEND_API_KEY);
const connection = new Connection(SOLANA_RPC_URL, "confirmed");

// Mock IDL Definition for the RWA Private Credit Protocol
const IDL = {
  version: "0.1.0",
  name: "rwa_credit_supercycle",
  instructions: [],
  events: [
    {
      name: "PoolInitializedEvent",
      fields: [
        { name: "pool", type: "publicKey", index: false },
        { name: "admin", type: "publicKey", index: false },
        { name: "borrower", type: "publicKey", index: false },
        { name: "capacity", type: "u64", index: false },
        { name: "seniorCapacity", type: "u64", index: false },
        { name: "juniorCapacity", type: "u64", index: false }
      ]
    },
    {
      name: "DepositEvent",
      fields: [
        { name: "pool", type: "publicKey", index: false },
        { name: "investor", type: "publicKey", index: false },
        { name: "amount", type: "u64", index: false },
        { name: "isSenior", type: "bool", index: false }
      ]
    },
    {
      name: "WaterfallRepaymentEvent",
      fields: [
        { name: "pool", type: "publicKey", index: false },
        { name: "totalRepayment", type: "u64", index: false },
        { name: "seniorInterest", type: "u64", index: false },
        { name: "juniorInterest", type: "u64", index: false },
        { name: "principalRepaid", type: "u64", index: false }
      ]
    }
  ]
};

export class EnterpriseRwaClient {
  private provider: AnchorProvider;
  private program: Program;

  constructor(payerKeypair: Keypair) {
    const wallet = new Wallet(payerKeypair);
    this.provider = new AnchorProvider(connection, wallet, { commitment: "confirmed" });
    this.program = new Program(IDL as any, PROGRAM_ID, this.provider);
  }

  /**
   * Dispatches Discord Alert to Institutional Operations Channel
   */
  async notifyDiscord(title: string, message: string, color: number = 0x00ff00): Promise<void> {
    if (!DISCORD_WEBHOOK_URL) {
      console.log(`[DISCORD DISABLED] ${title}: ${message}`);
      return;
    }
    try {
      await axios.post(DISCORD_WEBHOOK_URL, {
        embeds: [
          {
            title: `🏦 [RWA Credit Telemetry] ${title}`,
            description: message,
            color: color,
            timestamp: new Date().toISOString(),
            footer: { text: "Solana Tokenization Supercycle Engine" }
          }
        ]
      });
    } catch (err: any) {
      console.error("Failed to dispatch Discord telemetry:", err.message);
    }
  }

  /**
   * Dispatches Institutional Statement via Resend Email API
   */
  async sendInstitutionalEmail(
    recipient: string,
    subject: string,
    htmlContent: string
  ): Promise<void> {
    try {
      const response = await resend.emails.send({
        from: "institutional-alerts@rwasupercycle.solana.com",
        to: recipient,
        subject: `[SOLANA RWA] ${subject}`,
        html: htmlContent
      });
      console.log(`[RESEND SUCCESS] Message ID: ${response.data?.id} dispatched to ${recipient}`);
    } catch (err: any) {
      console.warn(`[RESEND NOTIFICATION FAILED] Reason: ${err.message}`);
    }
  }

  /**
   * Starts Real-Time Listener for On-Chain Credit Pool Events
   */
  async startEventListener(): Promise<void> {
    console.log("🚀 Initializing Solana RWA Telemetry Daemon...");
    console.log(`📡 Connecting to RPC: ${SOLANA_RPC_URL}`);
    console.log(`🔒 Target Program ID: ${PROGRAM_ID.toBase58()}`);

    // Listen to all program logs
    connection.onLogs(PROGRAM_ID, async (logs, ctx) => {
      if (logs.err) return;

      for (const log of logs.logs) {
        if (log.includes("DepositEvent")) {
          await this.handleDepositLog(log, logs.signature);
        } else if (log.includes("WaterfallRepaymentEvent")) {
          await this.handleRepaymentLog(log, logs.signature);
        }
      }
    });

    console.log("✅ WebSocket Listener Online. Monitoring pool state transitions in real time.");
  }

  private async handleDepositLog(logString: string, signature: string): Promise<void> {
    const notice = `📥 **New Tranche Liquidity Injection**\n• TX: \`${signature}\`\n• Log: ${logString}`;
    await this.notifyDiscord("Institutional Deposit Confirmed", notice, 0x3498db);

    await this.sendInstitutionalEmail(
      "lp-syndicate@fund.capital",
      "Notice of Capital Deployment Confirmation",
      `<h2>Institutional Private Credit Deposit</h2>
       <p>A new tranche deposit has been confirmed on-chain on Solana.</p>
       <p><strong>Transaction Signature:</strong> ${signature}</p>
       <p><strong>Timestamp:</strong> ${new Date().toUTCString()}</p>`
    );
  }

  private async handleRepaymentLog(logString: string, signature: string): Promise<void> {
    const notice = `💵 **Waterfall Yield & Principal Repayment Serviced**\n• TX: \`${signature}\`\n• Log: ${logString}`;
    await this.notifyDiscord("Waterfall Debt Repayment Processed", notice, 0x2ecc71);

    await this.sendInstitutionalEmail(
      "portfolio-managers@fund.capital",
      "Debt Distribution Waterfall Statement",
      `<h2>Yield Distribution Complete</h2>
       <p>Borrower has settled debt obligations across Senior and Junior Tranches.</p>
       <p><strong>Transaction Signature:</strong> ${signature}</p>`
    );
  }
}

// Daemon Bootstrap Entrypoint
if (require.main === module) {
  const dummyPayer = Keypair.generate();
  const client = new EnterpriseRwaClient(dummyPayer);
  client.startEventListener().catch((err) => {
    console.error("Fatal Daemon Error:", err);
    process.exit(1);
  });
}