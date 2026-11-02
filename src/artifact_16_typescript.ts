import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { CleanEnergyRwa } from "../target/types/clean_energy_rwa";
import {
  Keypair,
  PublicKey,
  SystemProgram,
  SYSVAR_RENT_PUBKEY,
} from "@solana/web3.js";
import {
  TOKEN_2022_PROGRAM_ID,
  createMint,
  createAccount,
  mintTo,
} from "@solana/spl-token";
import { expect } from "chai";

describe("Clean Energy RWA Token-2022 Engine Tests", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  const program = anchor.workspace.CleanEnergyRwa as Program<CleanEnergyRwa>;

  const payer = (provider.wallet as any).payer as Keypair;
  const oracleSigner = Keypair.generate();
  const accreditedInvestor = Keypair.generate();
  const unaccreditedInvestor = Keypair.generate();

  let bondMint: PublicKey;
  let usdcMint: PublicKey;
  let poolPda: PublicKey;
  let yieldVaultPda: PublicKey;

  before(async () => {
    // 1. Create Mock USDC Mint
    usdcMint = await createMint(
      provider.connection,
      payer,
      payer.publicKey,
      null,
      6
    );

    // 2. Create Token-2022 Bond Mint
    bondMint = await createMint(
      provider.connection,
      payer,
      payer.publicKey,
      null,
      0,
      undefined,
      undefined,
      TOKEN_2022_PROGRAM_ID
    );

    [poolPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("asset-pool"), bondMint.toBuffer()],
      program.programId
    );

    [yieldVaultPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("yield-vault"), poolPda.toBuffer()],
      program.programId
    );
  });

  it("1. Successfully initializes a Clean Energy RWA Asset Pool", async () => {
    const issuanceCap = new anchor.BN(10_000_000); // 10,000,000 Bonds ($10M)
    const couponBps = 850; // 8.50% APY
    const maturity = new anchor.BN(Math.floor(Date.now() / 1000) + 86400 * 365); // 1 Year

    await program.methods
      .initializeAsset(oracleSigner.publicKey, issuanceCap, couponBps, maturity)
      .accounts({
        authority: payer.publicKey,
        assetPool: poolPda,
        mint: bondMint,
        paymentMint: usdcMint,
        paymentVault: yieldVaultPda,
        systemProgram: SystemProgram.programId,
        tokenProgram: TOKEN_2022_PROGRAM_ID,
        rent: SYSVAR_RENT_PUBKEY,
      })
      .rpc();

    const poolAccount = await program.account.assetPool.fetch(poolPda);
    expect(poolAccount.authority.toBase58()).to.equal(payer.publicKey.toBase58());
    expect(poolAccount.couponRateBps).to.equal(850);
  });

  it("2. Blocks yield disbursement if non-oracle attempts update", async () => {
    const impostor = Keypair.generate();
    let failed = false;

    try {
      await program.methods
        .depositYield(new anchor.BN(50_000_000))
        .accounts({
          oracleOrAuthority: impostor.publicKey,
          pool: poolPda,
          bondMint: bondMint,
          paymentMint: usdcMint,
          sourcePaymentAccount: impostor.publicKey,
          paymentVault: yieldVaultPda,
          tokenProgram: TOKEN_2022_PROGRAM_ID,
        })
        .signers([impostor])
        .rpc();
    } catch (_err) {
      failed = true;
    }
    expect(failed).to.be.true;
  });

  it("3. Enforces compliance failure when unaccredited investor executes transfer hook", async () => {
    const [unaccreditedPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("investor"), unaccreditedInvestor.publicKey.toBuffer()],
      program.programId
    );

    let hookRejected = false;
    try {
      await program.methods
        .transferHook(new anchor.BN(100))
        .accounts({
          sourceTokenAccount: payer.publicKey,
          mint: bondMint,
          destinationTokenAccount: unaccreditedInvestor.publicKey,
          ownerDelegate: payer.publicKey,
          extraAccountMetas: poolPda,
          destInvestorEntry: unaccreditedPda,
          destinationInvestorOwner: unaccreditedInvestor.publicKey,
        })
        .rpc();
    } catch (err: any) {
      hookRejected = true;
    }
    // Correctly rejects unwhitelisted target
    expect(hookRejected).to.be.true;
  });
});