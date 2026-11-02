import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { CreditSupercycle } from "../target/types/credit_supercycle";
import {
  Keypair,
  PublicKey,
  SystemProgram,
  Transaction,
} from "@solana/web3.js";
import {
  TOKEN_2022_PROGRAM_ID,
  createMint,
  createAccount,
  mintTo,
  getAccount,
} from "@solana/spl-token";
import { expect } from "chai";

describe("Credit Supercycle RWA Tokenization Engine", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  const program = anchor.workspace.CreditSupercycle as Program<CreditSupercycle>;

  const originator = Keypair.generate();
  const accreditedInvestor = Keypair.generate();
  const unaccreditedInvestor = Keypair.generate();

  let paymentMint: PublicKey;
  let debtTokenMint: PublicKey;
  let creditPoolPda: PublicKey;
  let poolBump: number;
  let complianceRegistryPda: PublicKey;
  let registryBump: number;
  let vaultAccount: Keypair;

  let investorPaymentAccount: PublicKey;
  let investorDebtAccount: PublicKey;

  const TRANCHE_SIZE = new anchor.BN(1_000_000_000); // 1,000 USDC (6 decimals)
  const COUPON_RATE = 850; // 8.5%
  const MATURITY = Math.floor(Date.now() / 1000) + 86400 * 365; // 1 Year

  before(async () => {
    // Airdrop SOL to test actors
    for (const actor of [originator, accreditedInvestor, unaccreditedInvestor]) {
      const sig = await provider.connection.requestAirdrop(actor.publicKey, 10 * anchor.web3.LAMPORTS_PER_SOL);
      await provider.connection.confirmTransaction(sig);
    }

    // Deploy mock USDC payment mint
    paymentMint = await createMint(
      provider.connection,
      originator,
      originator.publicKey,
      null,
      6
    );

    // Deploy mock Token-2022 Tranche Debt Token Mint
    const debtMintKp = Keypair.generate();
    debtTokenMint = await createMint(
      provider.connection,
      originator,
      originator.publicKey,
      null,
      6,
      debtMintKp,
      undefined,
      TOKEN_2022_PROGRAM_ID
    );

    // Derive PDAs
    [creditPoolPda, poolBump] = PublicKey.findProgramAddressSync(
      [Buffer.from("credit_pool"), debtTokenMint.toBuffer()],
      program.programId
    );

    [complianceRegistryPda, registryBump] = PublicKey.findProgramAddressSync(
      [Buffer.from("compliance_registry"), creditPoolPda.toBuffer()],
      program.programId
    );

    vaultAccount = Keypair.generate();

    // Create investor stablecoin account and fund it
    investorPaymentAccount = await createAccount(
      provider.connection,
      accreditedInvestor,
      paymentMint,
      accreditedInvestor.publicKey
    );
    await mintTo(
      provider.connection,
      originator,
      paymentMint,
      investorPaymentAccount,
      originator,
      500_000_000
    );

    // Create investor debt token account
    investorDebtAccount = await createAccount(
      provider.connection,
      accreditedInvestor,
      debtTokenMint,
      accreditedInvestor.publicKey,
      undefined,
      undefined,
      TOKEN_2022_PROGRAM_ID
    );
  });

  it("1. Initializes Credit Pool with compliant Token-2022 configurations", async () => {
    await program.methods
      .initializePool(TRANCHE_SIZE, COUPON_RATE, new anchor.BN(MATURITY))
      .accounts({
        originator: originator.publicKey,
        creditPool: creditPoolPda,
        complianceRegistry: complianceRegistryPda,
        tokenMint: debtTokenMint,
        paymentMint: paymentMint,
        vault: vaultAccount.publicKey,
        systemProgram: SystemProgram.programId,
        tokenProgram: TOKEN_2022_PROGRAM_ID,
      })
      .signers([originator, vaultAccount])
      .rpc();

    const pool = await program.account.creditPool.fetch(creditPoolPda);
    expect(pool.originator.toBase58()).to.equal(originator.publicKey.toBase58());
    expect(pool.totalTrancheSize.toNumber()).to.equal(TRANCHE_SIZE.toNumber());
    expect(pool.couponRateBps).to.equal(850);
  });

  it("2. Originator whitelists accredited investor", async () => {
    const [investorRecordPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("investor_record"), complianceRegistryPda.toBuffer(), accreditedInvestor.publicKey.toBuffer()],
      program.programId
    );

    const validUntil = new anchor.BN(Math.floor(Date.now() / 1000) + 86400 * 180); // 180 days

    await program.methods
      .registerInvestor(accreditedInvestor.publicKey, 840, validUntil)
      .accounts({
        authority: originator.publicKey,
        complianceRegistry: complianceRegistryPda,
        investorRecord: investorRecordPda,
        systemProgram: SystemProgram.programId,
      })
      .signers([originator])
      .rpc();

    const record = await program.account.investorRecord.fetch(investorRecordPda);
    expect(record.isAccredited).to.be.true;
    expect(record.jurisdictionCode).to.equal(840);
  });

  it("3. Compliant investor deposits stablecoins and mints tranche tokens", async () => {
    const depositAmount = new anchor.BN(250_000_000); // 250 USDC
    const [investorRecordPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("investor_record"), complianceRegistryPda.toBuffer(), accreditedInvestor.publicKey.toBuffer()],
      program.programId
    );

    await program.methods
      .depositAndMint(depositAmount)
      .accounts({
        investor: accreditedInvestor.publicKey,
        creditPool: creditPoolPda,
        complianceRegistry: complianceRegistryPda,
        investorRecord: investorRecordPda,
        tokenMint: debtTokenMint,
        paymentMint: paymentMint,
        investorPaymentAccount: investorPaymentAccount,
        investorDebtAccount: investorDebtAccount,
        vault: vaultAccount.publicKey,
        tokenProgram: TOKEN_2022_PROGRAM_ID,
        paymentTokenProgram: anchor.spl.token.TOKEN_PROGRAM_ID,
      })
      .signers([accreditedInvestor])
      .rpc();

    const pool = await program.account.creditPool.fetch(creditPoolPda);
    expect(pool.totalBorrowed.toNumber()).to.equal(depositAmount.toNumber());

    const debtTokenAccount = await getAccount(
      provider.connection,
      investorDebtAccount,
      undefined,
      TOKEN_2022_PROGRAM_ID
    );
    expect(Number(debtTokenAccount.amount)).to.equal(depositAmount.toNumber());
  });

  it("4. Blocks transfer hook for unaccredited recipient", async () => {
    const [unaccreditedRecordPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("investor_record"), complianceRegistryPda.toBuffer(), unaccreditedInvestor.publicKey.toBuffer()],
      program.programId
    );

    try {
      await program.methods
        .executeHook(new anchor.BN(100_000))
        .accounts({
          sourceTokenAccount: investorDebtAccount,
          mint: debtTokenMint,
          destinationTokenAccount: investorDebtAccount, // arbitrary mock
          destinationOwner: unaccreditedInvestor.publicKey,
          complianceRegistry: complianceRegistryPda,
          recipientInvestorRecord: unaccreditedRecordPda,
        })
        .rpc();
      expect.fail("Transfer should have been blocked by Transfer Hook");
    } catch (err: any) {
      expect(err.toString()).to.include("AccountNotInitialized");
    }
  });

  it("5. Yield distribution increments pool global index", async () => {
    const yieldPayment = new anchor.BN(21_250_000); // Repayment
    const originatorPaymentAccount = await createAccount(
      provider.connection,
      originator,
      paymentMint,
      originator.publicKey
    );
    await mintTo(
      provider.connection,
      originator,
      paymentMint,
      originatorPaymentAccount,
      originator,
      100_000_000
    );

    await program.methods
      .distributeYield(yieldPayment)
      .accounts({
        originator: originator.publicKey,
        pool: creditPoolPda,
        paymentMint: paymentMint,
        originatorPaymentAccount: originatorPaymentAccount,
        vault: vaultAccount.publicKey,
        paymentTokenProgram: anchor.spl.token.TOKEN_PROGRAM_ID,
      })
      .signers([originator])
      .rpc();

    const pool = await program.account.creditPool.fetch(creditPoolPda);
    expect(pool.totalYieldRepaid.toNumber()).to.equal(yieldPayment.toNumber());
    expect(pool.poolIndex.gt(new anchor.BN(1_000_000_000_000_000_000u128))).to.be.true;
  });
});