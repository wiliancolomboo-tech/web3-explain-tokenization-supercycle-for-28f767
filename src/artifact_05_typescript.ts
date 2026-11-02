// File: tests/sol_rwa_credit.ts

import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { Keypair, PublicKey, SystemProgram } from "@solana/web3.js";
import { 
  TOKEN_PROGRAM_ID, 
  createMint, 
  createAccount, 
  mintTo, 
  getAccount 
} from "@solana/spl-token";
import { expect } from "chai";
import * as crypto from "crypto";

describe("SolRWA Private Credit: Tokenization Supercycle Engine", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  const program = anchor.workspace.SolRwaCredit as Program;

  const authority = Keypair.generate();
  const servicer = Keypair.generate();
  const accreditedInvestor = Keypair.generate();
  const unauthorizedInvestor = Keypair.generate();

  let usdcMint: PublicKey;
  let authorityUsdc: PublicKey;
  let servicerUsdc: PublicKey;
  let investorUsdc: PublicKey;

  const poolId = "FACILITY-2025-INV01";
  const targetPrincipal = new anchor.BN(1_000_000 * 1e6); // 1,000,000 USDC
  const seniorRatioBps = new anchor.BN(8000); // 80% Senior Tranche ($800k)
  const tenorSeconds = new anchor.BN(30 * 86400); // 30 Days

  let poolPda: PublicKey;
  let seniorMintPda: PublicKey;
  let juniorMintPda: PublicKey;
  let vaultPda: PublicKey;
  let investorRecordPda: PublicKey;
  let investorSeniorAta: PublicKey;

  before(async () => {
    // Fund test wallets
    const airdropSigners = [authority, servicer, accreditedInvestor, unauthorizedInvestor];
    for (const signer of airdropSigners) {
      const sig = await provider.connection.requestAirdrop(signer.publicKey, 10 * anchor.web3.LAMPORTS_PER_SOL);
      await provider.connection.confirmTransaction(sig);
    }

    // Create Mock USDC Mint (6 Decimals)
    usdcMint = await createMint(
      provider.connection,
      authority,
      authority.publicKey,
      null,
      6
    );

    authorityUsdc = await createAccount(provider.connection, authority, usdcMint, authority.publicKey);
    servicerUsdc = await createAccount(provider.connection, servicer, usdcMint, servicer.publicKey);
    investorUsdc = await createAccount(provider.connection, accreditedInvestor, usdcMint, accreditedInvestor.publicKey);

    // Mint test liquidity
    await mintTo(provider.connection, authority, usdcMint, investorUsdc, authority, 1_000_000 * 1e6);
    await mintTo(provider.connection, authority, usdcMint, servicerUsdc, authority, 1_050_000 * 1e6);

    // Derive PDA addresses
    [poolPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("pool"), Buffer.from(poolId)],
      program.programId
    );
    [seniorMintPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("senior"), Buffer.from(poolId)],
      program.programId
    );
    [juniorMintPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("junior"), Buffer.from(poolId)],
      program.programId
    );
    [vaultPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("vault"), Buffer.from(poolId)],
      program.programId
    );
    [investorRecordPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("investor"), accreditedInvestor.publicKey.toBuffer()],
      program.programId
    );
  });

  it("1. Successfully initializes a structured private credit pool with tranches", async () => {
    await program.methods
      .initializePool(poolId, targetPrincipal, tenorSeconds, seniorRatioBps)
      .accounts({
        pool: poolPda,
        authority: authority.publicKey,
        usdcMint: usdcMint,
        seniorMint: seniorMintPda,
        juniorMint: juniorMintPda,
        poolVault: vaultPda,
        systemProgram: SystemProgram.programId,
        tokenProgram: TOKEN_PROGRAM_ID,
        rent: anchor.web3.SYSVAR_RENT_PUBKEY,
      })
      .signers([authority])
      .rpc();

    const poolAccount = await program.account.creditPool.fetch(poolPda);
    expect(poolAccount.poolId).to.equal(poolId);
    expect(poolAccount.principalTarget.toNumber()).to.equal(1_000_000 * 1e6);
    expect(poolAccount.seniorRatioBps.toNumber()).to.equal(8000);
    expect(poolAccount.isFunded).to.be.false;
  });

  it("2. Registers and verifies accredited investor identity via KYC hash", async () => {
    const validUntil = new anchor.BN(Math.floor(Date.now() / 1000) + 365 * 86400);
    const kycHash = crypto.createHash("sha256").update("Institutional Hedge Fund LLC").digest();

    await program.methods
      .registerInvestor(accreditedInvestor.publicKey, validUntil, Array.from(kycHash))
      .accounts({
        authority: authority.publicKey,
        investorRecord: investorRecordPda,
        investorAddress: accreditedInvestor.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .signers([authority])
      .rpc();

    const record = await program.account.investorRecord.fetch(investorRecordPda);
    expect(record.investor.toBase58()).to.equal(accreditedInvestor.publicKey.toBase58());
    expect(record.isBlacklisted).to.be.false;
  });

  it("3. Prevents an unregistered / non-accredited investor from subscribing capital", async () => {
    const [unregRecordPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("investor"), unauthorizedInvestor.publicKey.toBuffer()],
      program.programId
    );

    const unauthorizedAta = await createAccount(
      provider.connection,
      unauthorizedInvestor,
      seniorMintPda,
      unauthorizedInvestor.publicKey
    );

    let failed = false;
    try {
      await program.methods
        .depositAndMint(new anchor.BN(10_000 * 1e6), true)
        .accounts({
          investor: unauthorizedInvestor.publicKey,
          pool: poolPda,
          investorRecord: unregRecordPda,
          usdcMint: usdcMint,
          seniorMint: seniorMintPda,
          juniorMint: juniorMintPda,
          investorUsdc: investorUsdc,
          poolVault: vaultPda,
          investorTrancheToken: unauthorizedAta,
          tokenProgram: TOKEN_PROGRAM_ID,
          systemProgram: SystemProgram.programId,
        })
        .signers([unauthorizedInvestor])
        .rpc();
    } catch (e: any) {
      failed = true;
      expect(e.toString()).to.include("AccountNotInitialized");
    }
    expect(failed).to.be.true;
  });

  it("4. Allows accredited investor to deposit USDC and mint Senior Tranche tokens", async () => {
    investorSeniorAta = await createAccount(
      provider.connection,
      accreditedInvestor,
      seniorMintPda,
      accreditedInvestor.publicKey
    );

    const depositAmount = new anchor.BN(800_000 * 1e6); // Fill senior tranche (80%)

    await program.methods
      .depositAndMint(depositAmount, true)
      .accounts({
        investor: accreditedInvestor.publicKey,
        pool: poolPda,
        investorRecord: investorRecordPda,
        usdcMint: usdcMint,
        seniorMint: seniorMintPda,
        juniorMint: juniorMintPda,
        investorUsdc: investorUsdc,
        poolVault: vaultPda,
        investorTrancheToken: investorSeniorAta,
        tokenProgram: TOKEN_PROGRAM_ID,
        systemProgram: SystemProgram.programId,
      })
      .signers([accreditedInvestor])
      .rpc();

    const seniorAtaBalance = await getAccount(provider.connection, investorSeniorAta);
    expect(Number(seniorAtaBalance.amount)).to.equal(800_000 * 1e6);
  });

  it("5. Prevents exceeding tranche allocation capacity", async () => {
    let failed = false;
    try {
      // Attempt to deposit $1 over the $800k senior target
      await program.methods
        .depositAndMint(new anchor.BN(1 * 1e6), true)
        .accounts({
          investor: accreditedInvestor.publicKey,
          pool: poolPda,
          investorRecord: investorRecordPda,
          usdcMint: usdcMint,
          seniorMint: seniorMintPda,
          juniorMint: juniorMintPda,
          investorUsdc: investorUsdc,
          poolVault: vaultPda,
          investorTrancheToken: investorSeniorAta,
          tokenProgram: TOKEN_PROGRAM_ID,
          systemProgram: SystemProgram.programId,
        })
        .signers([accreditedInvestor])
        .rpc();
    } catch (err: any) {
      failed = true;
      expect(err.error.errorCode.code).to.equal("TrancheCapacityExceeded");
    }
    expect(failed).to.be.true;
  });

  it("6. Fills Junior tranche, transitions pool to funded, and executes repayment waterfall", async () => {
    const investorJuniorAta = await createAccount(
      provider.connection,
      accreditedInvestor,
      juniorMintPda,
      accreditedInvestor.publicKey
    );

    const juniorDeposit = new anchor.BN(200_000 * 1e6); // Fill remaining 20%
    await program.methods
      .depositAndMint(juniorDeposit, false)
      .accounts({
        investor: accreditedInvestor.publicKey,
        pool: poolPda,
        investorRecord: investorRecordPda,
        usdcMint: usdcMint,
        seniorMint: seniorMintPda,
        juniorMint: juniorMintPda,
        investorUsdc: investorUsdc,
        poolVault: vaultPda,
        investorTrancheToken: investorJuniorAta,
        tokenProgram: TOKEN_PROGRAM_ID,
        systemProgram: SystemProgram.programId,
      })
      .signers([accreditedInvestor])
      .rpc();

    const pool = await program.account.creditPool.fetch(poolPda);
    expect(pool.isFunded).to.be.true;
    expect(pool.totalFunded.toNumber()).to.equal(1_000_000 * 1e6);

    // Servicer completes loan repayment with 5% yield over tenor
    const totalRepay = new anchor.BN(1_050_000 * 1e6);
    await program.methods
      .processRepaymentWaterfall(totalRepay)
      .accounts({
        servicer: servicer.publicKey,
        pool: poolPda,
        usdcMint: usdcMint,
        seniorMint: seniorMintPda,
        poolVault: vaultPda,
        servicerUsdc: servicerUsdc,
        vault: vaultPda,
        tokenProgram: TOKEN_PROGRAM_ID,
      })
      .signers([servicer])
      .rpc();

    const updatedPool = await program.account.creditPool.fetch(poolPda);
    expect(updatedPool.repaidAmount.toNumber()).to.equal(1_050_000 * 1e6);
  });
});