import { Connection, Keypair, PublicKey } from "@solana/web3.js";
import { Program, AnchorProvider, Wallet } from "@coral-xyz/anchor";
import { CONFIG } from "../config";
import fs from "fs";

export class SolanaRwaClient {
  public connection: Connection;
  public provider: AnchorProvider;
  public keypair: Keypair;
  public programId: PublicKey;

  constructor() {
    this.connection = new Connection(CONFIG.SOLANA_RPC_URL, {
      wsEndpoint: CONFIG.SOLANA_WS_URL,
      commitment: "confirmed",
    });

    if (CONFIG.AUTHORITY_PRIVATE_KEY.startsWith("[")) {
      this.keypair = Keypair.fromSecretKey(
        Uint8Array.from(JSON.parse(CONFIG.AUTHORITY_PRIVATE_KEY))
      );
    } else if (fs.existsSync(CONFIG.AUTHORITY_PRIVATE_KEY)) {
      const secret = JSON.parse(fs.readFileSync(CONFIG.AUTHORITY_PRIVATE_KEY, "utf-8"));
      this.keypair = Keypair.fromSecretKey(Uint8Array.from(secret));
    } else {
      this.keypair = Keypair.generate();
    }

    const wallet = new Wallet(this.keypair);
    this.provider = new AnchorProvider(this.connection, wallet, {
      commitment: "confirmed",
      preflightCommitment: "confirmed",
    });
    this.programId = new PublicKey(CONFIG.PROGRAM_ID);
  }

  public getInvestorIdentityPDA(investor: PublicKey): [PublicKey, number] {
    return PublicKey.findProgramAddressSync(
      [Buffer.from("investor_identity"), investor.toBuffer()],
      this.programId
    );
  }

  public getAssetConfigPDA(mint: PublicKey): [PublicKey, number] {
    return PublicKey.findProgramAddressSync(
      [Buffer.from("asset_config"), mint.toBuffer()],
      this.programId
    );
  }
}