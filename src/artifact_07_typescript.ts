import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { Keypair, PublicKey, SystemProgram, SYSVAR_RENT_PUBKEY } from "@solana/web3.js";
import { TOKEN_2022_PROGRAM_ID, createMint, createAccount, mintTo } from "@solana/spl-token";
import { expect } from "chai";
import { RwaCreditSupercycle } from "../target/types/rwa_credit_supercycle";

describe("Solana RWA Tokenization Supercycle Integration Tests", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  const program = anchor.workspace.RwaCreditSupercycle as Program<RwaCreditSupercycle>;

  const admin = Keypair.generate();
  const borrower = Keypair.generate();
  const investorSenior = Keypair.generate();
  const investorJunior = Keypair.generate();
  const unverifiedInvestor = Keypair.generate();

  let usdcMint: PublicKey;
  let poolPda: PublicKey;
  let poolBump: number;
  let seniorMintPda: PublicKey;
  let juniorMintPda: PublicKey;
  let usdcVaultPda: PublicKey;

  const POOL_NAME = "Series-A-Receivables-2025";
  const CAPACITY = new anchor.BN(1_000_000_000); // 1,000 USDC
  const SENIOR_BPS = new anchor.BN(8000); // 80% Senior / 20% Junior
  const DURATION = new anchor.BN(365 * 86400);

  before(async () => {
    // Fund Admin & Participants
    for (const kp of [admin, borrower, investorSenior, investorJunior, unverifiedInvestor]) {
      const sig = await provider.connection.requestAirdrop(kp.publicKey, 10 * anchor.web3.LAMPORTS_PER_SOL);
      await provider.connection.confirmTransaction(sig);
    }

    // Create Mock Underlying USDC Mint
    usdcMint = await createMint(
      provider.connection,
      admin,
      admin.publicKey,
      null,
      6,
      Keypair.generate(),
      undefined,
      TOKEN_2022_PROGRAM_ID
    );

    // Derive Program PDAs
    [poolPda, poolBump] = PublicKey.findProgramAddressSync(
      [Buffer.from("credit_pool"), admin.publicKey.toBuffer(), Buffer.from(POOL_NAME)],
      program.programId
    );

    [seniorMintPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("senior_mint"), poolPda.toBuffer()],
      program.programId
    );

    [juniorMintPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("junior_mint"), poolPda.toBuffer()],
      program.programId
    );

    [usdcVaultPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("usdc_vault"), poolPda.toBuffer()],
      program.programId
    );
  });

  it("1. Initializes the Institutional Credit Pool with 80/20 Tranche Architecture", async () => {
    const tx = await program.methods
      .initializePool(POOL_NAME, CAPACITY, DURATION, SENIOR_BPS)
      .accounts({
        pool: poolPda,
        admin: admin.publicKey,
        borrower: borrower.publicKey,
        usdcMint: usdcMint,
        seniorMint: seniorMintPda,
        juniorMint: juniorMintPda,
        usdcVault: usdcVaultPda,
        systemProgram: SystemProgram.programId,
        token2022Program: TOKEN_2022_PROGRAM_ID,
        rent: SYSVAR_RENT_PUBKEY,
      })
      .signers([admin])
      .rpc();

    expect(tx).to.be.a("string");
    const poolAccount = await program.account.creditPool.fetch(poolPda);
    expect(poolAccount.seniorCapacity.toNumber()).to.equal(800_000_000);
    expect(poolAccount.juniorCapacity.toNumber()).to.equal(200_000_000);
    expect(poolAccount.poolName).to.equal(POOL_NAME);
  });

  it("2. Sets KYC Passport Status for Accredited Institutional Lenders", async () => {
    for (const lender of [investorSenior, investorJunior]) {
      const [kycPda] = PublicKey.findProgramAddressSync(
        [Buffer.from("kyc_registry"), lender.publicKey.toBuffer()],
        program.programId
      );

      await program.methods
        .setKycStatus(lender.publicKey, true, [85, 83]) // "US"
        .accounts({
          kycRegistry: kycPda,
          investor: lender.publicKey,
          authority: admin.publicKey,
          systemProgram: SystemProgram.programId,
        })
        .signers([admin])
        .rpc();

      const kycAccount = await program.account.kycRegistry.fetch(kycPda);
      expect(kycAccount.isKyced).to.be.true;
    }
  });

  it("3. Blocks Unverified Non-KYC Investor Deposits Deterministically", async () => {
    const [unverifiedKycPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("kyc_registry"), unverifiedInvestor.publicKey.toBuffer()],
      program.programId
    );

    // Explicitly set is_kyced = false
    await program.methods
      .setKycStatus(unverifiedInvestor.publicKey, false, [0, 0])
      .accounts({
        kycRegistry: unverifiedKycPda,
        investor: unverifiedInvestor.publicKey,
        authority: admin.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .signers([admin])
      .rpc();

    const fakeTokenAccount = await createAccount(
      provider.connection,
      unverifiedInvestor,
      usdcMint,
      unverifiedInvestor.publicKey,
      Keypair.generate(),
      undefined,
      TOKEN_2022_PROGRAM_ID
    );

    try {
      await program.methods
        .depositTranche(new anchor.BN(100_000_000), true)
        .accounts({
          pool: poolPda,
          kycRegistry: unverifiedKycPda,
          investor: unverifiedInvestor.publicKey,
          investorUsdc: fakeTokenAccount,
          usdcVault: usdcVaultPda,
          usdcMint: usdcMint,
          seniorMint: seniorMintPda,
          juniorMint: juniorMintPda,
          investorTrancheToken: fakeTokenAccount,
          token2022Program: TOKEN_2022_PROGRAM_ID,
        })
        .signers([unverifiedInvestor])
        .rpc();
      expect.fail("Transaction should have reverted with KycRequired");
    } catch (err: any) {
      expect(err.error.errorCode.code).to.equal("KycRequired");
    }
  });

  it("4. Accepts Senior and Junior Tranche Deposits from Compliant Lenders", async () => {
    // Create USDC ATA & Mint Tokens for Senior Investor
    const investorSeniorUsdc = await createAccount(
      provider.connection,
      investorSenior,
      usdcMint,
      investorSenior.publicKey,
      Keypair.generate(),
      undefined,
      TOKEN_2022_PROGRAM_ID
    );
    await mintTo(
      provider.connection,
      admin,
      usdcMint,
      investorSeniorUsdc,
      admin,
      800_000_000,
      [],
      undefined,
      TOKEN_2022_PROGRAM_ID
    );

    const investorSeniorTranche = await createAccount(
      provider.connection,
      investorSenior,
      seniorMintPda,
      investorSenior.publicKey,
      Keypair.generate(),
      undefined,
      TOKEN_2022_PROGRAM_ID
    );

    const [seniorKycPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("kyc_registry"), investorSenior.publicKey.toBuffer()],
      program.programId
    );

    await program.methods
      .depositTranche(new anchor.BN(800_000_000), true)
      .accounts({
        pool: poolPda,
        kycRegistry: seniorKycPda,
        investor: investorSenior.publicKey,
        investorUsdc: investorSeniorUsdc,
        usdcVault: usdcVaultPda,
        usdcMint: usdcMint,
        seniorMint: seniorMintPda,
        juniorMint: juniorMintPda,
        investorTrancheToken: investorSeniorTranche,
        token2022Program: TOKEN_2022_PROGRAM_ID,
      })
      .signers([investorSenior])
      .rpc();

    const pool = await program.account.creditPool.fetch(poolPda);
    expect(pool.seniorDeposited.toNumber()).to.equal(800_000_000);
  });
});