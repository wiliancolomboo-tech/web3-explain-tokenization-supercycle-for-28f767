import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { Keypair, PublicKey, SystemProgram } from "@solana/web3.js";
import {
  createMint,
  createAccount,
  mintTo,
  getAccount,
  TOKEN_2022_PROGRAM_ID,
} from "@solana/spl-token";
import { expect } from "chai";
import * as crypto from "crypto";

describe("Solana Token-2022 Invoice Credit Engine QA Matrix", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  const program = anchor.workspace.InvoiceCreditEngine as Program<any>;

  const originator = Keypair.generate();
  const investor = Keypair.generate();
  const debtorPayer = Keypair.generate();

  let stableMint: PublicKey;
  let trancheMintKeypair: Keypair;
  let originatorStableAccount: PublicKey;
  let investorStableAccount: PublicKey;
  let payerStableAccount: PublicKey;
  let investorTrancheAccount: PublicKey;

  const poolId = new anchor.BN(Date.now());
  const invoiceFaceValue = new anchor.BN(100_000_000_000); // 100,000.00 USDC
  const financingAmount = new anchor.BN(80_000_000_000);   // 80,000.00 USDC
  const discountRateBps = new anchor.BN(1200);             // 12.00% APR
  const durationSeconds = new anchor.BN(90 * 86400);       // 90 Days Tenor
  const invoiceHash = Array.from(crypto.createHash("sha256").update("INV-GLOBAL-998822").digest());

  let poolPda: PublicKey;
  let escrowPda: PublicKey;

  before(async () => {
    // 1. Airdrop SOL to participants
    for (const kp of [originator, investor, debtorPayer]) {
      const sig = await provider.connection.requestAirdrop(kp.publicKey, 10 * anchor.web3.LAMPORTS_PER_SOL);
      await provider.connection.confirmTransaction(sig);
    }

    // 2. Setup Mock USDC under Token-2022
    stableMint = await createMint(
      provider.connection,
      originator,
      originator.publicKey,
      null,
      6,
      undefined,
      undefined,
      TOKEN_2022_PROGRAM_ID
    );

    // 3. Create Stable token accounts
    originatorStableAccount = await createAccount(
      provider.connection,
      originator,
      stableMint,
      originator.publicKey,
      undefined,
      undefined,
      TOKEN_2022_PROGRAM_ID
    );

    investorStableAccount = await createAccount(
      provider.connection,
      investor,
      stableMint,
      investor.publicKey,
      undefined,
      undefined,
      TOKEN_2022_PROGRAM_ID
    );

    payerStableAccount = await createAccount(
      provider.connection,
      debtorPayer,
      stableMint,
      debtorPayer.publicKey,
      undefined,
      undefined,
      TOKEN_2022_PROGRAM_ID
    );

    // 4. Fund Investor & Debtor with stablecoins
    await mintTo(
      provider.connection,
      originator,
      stableMint,
      investorStableAccount,
      originator,
      100_000_000_000,
      [],
      undefined,
      TOKEN_2022_PROGRAM_ID
    );

    await mintTo(
      provider.connection,
      originator,
      stableMint,
      payerStableAccount,
      originator,
      120_000_000_000,
      [],
      undefined,
      TOKEN_2022_PROGRAM_ID
    );

    // Derive PDAs
    [poolPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("invoice_vault"), poolId.toArrayLike(Buffer, "le", 8)],
      program.programId
    );

    [escrowPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("escrow_vault"), poolId.toArrayLike(Buffer, "le", 8)],
      program.programId
    );

    trancheMintKeypair = Keypair.generate();
  });

  it("Test 1: Initializes the Private Credit Invoice Factoring Pool", async () => {
    await program.methods
      .initializePool(
        poolId,
        invoiceFaceValue,
        financingAmount,
        discountRateBps,
        durationSeconds,
        invoiceHash
      )
      .accounts({
        pool: poolPda,
        originator: originator.publicKey,
        stableMint: stableMint,
        trancheMint: trancheMintKeypair.publicKey,
        poolEscrow: escrowPda,
        systemProgram: SystemProgram.programId,
        tokenProgram: TOKEN_2022_PROGRAM_ID,
        rent: anchor.web3.SYSVAR_RENT_PUBKEY,
      })
      .signers([originator, trancheMintKeypair])
      .rpc();

    const poolAccount = await program.account.invoicePool.fetch(poolPda);
    expect(poolAccount.originator.toBase58()).to.equal(originator.publicKey.toBase58());
    expect(poolAccount.invoiceFaceValue.toString()).to.equal(invoiceFaceValue.toString());
    expect(poolAccount.financingAmount.toString()).to.equal(financingAmount.toString());
    expect(poolAccount.isSettled).to.be.false;
  });

  it("Test 2: Institutional Investor Supplies Liquidity & Receives Token-2022 Tranche Notes", async () => {
    // Create investor tranche token account
    investorTrancheAccount = await createAccount(
      provider.connection,
      investor,
      trancheMintKeypair.publicKey,
      investor.publicKey,
      undefined,
      undefined,
      TOKEN_2022_PROGRAM_ID
    );

    const supplyAmount = financingAmount; // Fund 100% of available capacity

    await program.methods
      .supplyLiquidity(supplyAmount)
      .accounts({
        pool: poolPda,
        investor: investor.publicKey,
        investorStableAccount: investorStableAccount,
        originatorStableAccount: originatorStableAccount,
        trancheMint: trancheMintKeypair.publicKey,
        investorTrancheAccount: investorTrancheAccount,
        stableMint: stableMint,
        tokenProgram: TOKEN_2022_PROGRAM_ID,
      })
      .signers([investor])
      .rpc();

    // Verify Originator received stablecoin advance
    const originatorBal = await getAccount(provider.connection, originatorStableAccount, undefined, TOKEN_2022_PROGRAM_ID);
    expect(originatorBal.amount.toString()).to.equal(supplyAmount.toString());

    // Verify Investor received exact Tranche tokens
    const trancheBal = await getAccount(provider.connection, investorTrancheAccount, undefined, TOKEN_2022_PROGRAM_ID);
    expect(trancheBal.amount.toString()).to.equal(supplyAmount.toString());
  });

  it("Test 3: Rejects Over-Subscription Beyond Advance Capacity", async () => {
    try {
      await program.methods
        .supplyLiquidity(new anchor.BN(10_000_000))
        .accounts({
          pool: poolPda,
          investor: investor.publicKey,
          investorStableAccount: investorStableAccount,
          originatorStableAccount: originatorStableAccount,
          trancheMint: trancheMintKeypair.publicKey,
          investorTrancheAccount: investorTrancheAccount,
          stableMint: stableMint,
          tokenProgram: TOKEN_2022_PROGRAM_ID,
        })
        .signers([investor])
        .rpc();
      expect.fail("Should have thrown DepositExceedsCapacity");
    } catch (err: any) {
      expect(err.error?.errorCode?.code ?? err.toString()).to.include("DepositExceedsCapacity");
    }
  });

  it("Test 4: Corporate Debtor Settles Invoice with Accrued Interest into Escrow", async () => {
    // Repayment: $80,000 principal + (~12% APR over 90 days = ~$2,367.12)
    const totalRepay = new anchor.BN(82_367_123_287);

    await program.methods
      .settleInvoice(totalRepay)
      .accounts({
        pool: poolPda,
        payer: debtorPayer.publicKey,
        payerStableAccount: payerStableAccount,
        poolEscrow: escrowPda,
        stableMint: stableMint,
        tokenProgram: TOKEN_2022_PROGRAM_ID,
      })
      .signers([debtorPayer])
      .rpc();

    const poolAccount = await program.account.invoicePool.fetch(poolPda);
    expect(poolAccount.isSettled).to.be.true;
    expect(poolAccount.repaidAmount.toString()).to.equal(totalRepay.toString());
  });

  it("Test 5: Senior LP Burns Tranche Notes to Redeem Principal & Yield", async () => {
    const trancheTokensToBurn = financingAmount;

    const investorStableBefore = await getAccount(provider.connection, investorStableAccount, undefined, TOKEN_2022_PROGRAM_ID);

    await program.methods
      .redeemYield(trancheTokensToBurn)
      .accounts({
        pool: poolPda,
        investor: investor.publicKey,
        investorTrancheAccount: investorTrancheAccount,
        trancheMint: trancheMintKeypair.publicKey,
        poolEscrow: escrowPda,
        investorStableAccount: investorStableAccount,
        stableMint: stableMint,
        tokenProgram: TOKEN_2022_PROGRAM_ID,
      })
      .signers([investor])
      .rpc();

    // Verify tranche tokens were burned
    const trancheBalAfter = await getAccount(provider.connection, investorTrancheAccount, undefined, TOKEN_2022_PROGRAM_ID);
    expect(trancheBalAfter.amount.toString()).to.equal("0");

    // Verify investor received principal + full interest
    const investorStableAfter = await getAccount(provider.connection, investorStableAccount, undefined, TOKEN_2022_PROGRAM_ID);
    const payoutReceived = BigInt(investorStableAfter.amount.toString()) - BigInt(investorStableBefore.amount.toString());
    expect(payoutReceived >= 82_367_123_287n).to.be.true;
  });
});