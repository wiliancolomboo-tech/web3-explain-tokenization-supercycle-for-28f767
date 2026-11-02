import * as anchor from "@coral-xyz/anchor";
import { Connection, PublicKey } from "@solana/web3.js";
import { sendEnterpriseAlert } from "./alertService";

export class RwaComplianceDaemon {
  private connection: Connection;
  private program: anchor.Program;
  private poolPubkey: PublicKey;

  constructor(rpcUrl: string, program: anchor.Program, poolPubkey: PublicKey) {
    this.connection = new Connection(rpcUrl, "confirmed");
    this.program = program;
    this.poolPubkey = poolPubkey;
  }

  public async startListening(): Promise<void> {
    console.log(`[DAEMON] Monitoring Compliance Events for Pool: ${this.poolPubkey.toBase58()}`);

    // Listen to Transfer Compliance Verified Event
    this.program.addEventListener("TransferComplianceVerifiedEvent", async (event, slot, signature) => {
      console.log(`[DAEMON] Compliance Hook Verified Transfer in Slot ${slot}. Tx: ${signature}`);
      await sendEnterpriseAlert({
        eventType: "TRANSFER_SUCCESS",
        poolId: this.poolPubkey.toBase58(),
        source: event.source.toBase58(),
        destination: event.destination.toBase58(),
        amount: event.amount.toString(),
        txSignature: signature,
      });
    });

    // Listen to Pool Freeze Events
    this.program.addEventListener("PoolFreezeToggledEvent", async (event, slot, signature) => {
      console.warn(`[DAEMON] Pool Freeze Status changed to: ${event.isFrozen}. Tx: ${signature}`);
      await sendEnterpriseAlert({
        eventType: "POOL_FROZEN",
        poolId: event.pool.toBase58(),
        reason: event.isFrozen ? "Emergency Admin Freeze Triggered" : "Emergency Admin Freeze Lifted",
        txSignature: signature,
      });
    });
  }
}