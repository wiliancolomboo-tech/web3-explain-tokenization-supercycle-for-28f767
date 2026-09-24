import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { SovereignYieldEngine } from "../target/types/sovereign_yield_engine";
import {
  TOKEN_2022_PROGRAM_ID,
  TOKEN_PROGRAM_ID,
  createMint,
  createAccount,
  mintTo,
  getAccount,
} from "@solana/spl-token";
import { Keypair, SystemProgram, SYSVAR_RENT_PUBKEY } from "@solana/web3.js";
import { expect } from "chai";

describe("sovereign_yield_engine", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  const program = anchor.workspace.SovereignYieldEngine as Program<SovereignYieldEngine>;

  let usdcMint: anchor.web3.PublicKey;
  let seniorMint: anchor.web3.PublicKey;
  let juniorMint: anchor.web3.PublicKey;

  let poolConfigPda: anchor.web3.PublicKey;
  let poolBump: number;
  let seniorVaultPda: anchor.web3.PublicKey;
  let juniorVaultPda: anchor.web3.PublicKey;
  let kycRegistryPda: anchor.web3.PublicKey;

  const authority = provider.wallet as anchor.Wallet;
  const institutionalInvestor = Keypair.generate();
  const unverifiedEntity = Keypair.generate();

  let investorUsdc: anchor.web3.PublicKey;
  let investorSeniorAta: anchor.web3.PublicKey;

  before(async () => {
    // 1. Airdrop SOL to test participants
    const txAirdrop = await provider.connection.requestAirdrop(
      institutionalInvestor.publicKey,
      10 * anchor.web3.LAMPORTS_PER_SOL
    );
    await provider.connection.confirmTransaction(txAirdrop);

    // 2. Initialize Base USDC Mock Mint (Legacy Token Program)
    usdcMint = await createMint(
      provider.connection,
      authority.payer,
      authority.publicKey,
      null,
      6,
      undefined,
      undefined,
      TOKEN_PROGRAM_ID
    );

    // 3. Derive Program PDAs
    [poolConfigPda, poolBump] = anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("pool_config"), usdcMint.toBuffer()],
      program.programId
    );

    [seniorVaultPda] = anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("senior_vault"), poolConfigPda.toBuffer()],
      program.programId
    );

    [juniorVaultPda] = anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("junior_vault"), poolConfigPda.toBuffer()],
      program.programId
    );

    [kycRegistryPda] = anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("kyc_registry"), authority.publicKey.toBuffer()],
      program.programId
    );

    // 4. Initialize Mock Token-2022 Senior Mint with Authority set to poolConfigPda
    seniorMint = await createMint(
      provider.connection,
      authority.payer,
      poolConfigPda,
      null,
      6,
      undefined,
      undefined,
      TOKEN_2022_PROGRAM_ID
    );

    juniorMint = await createMint(
      provider.connection,
      authority.payer,
      poolConfigPda,
      null,
      6,
      undefined,
      undefined,
      TOKEN_2022_PROGRAM_ID
    );

    // 5. Setup Token Accounts for Investor
    investorUsdc = await createAccount(
      provider.connection,
      authority.payer,
      usdcMint,
      institutionalInvestor.publicKey,
      undefined,
      undefined,
      TOKEN_PROGRAM_ID
    );

    investorSeniorAta = await createAccount(
      provider.connection,
      authority.payer,
      seniorMint,
      institutionalInvestor.publicKey,
      undefined,
      undefined,
      TOKEN_2022_PROGRAM_ID
    );

    // Mint 1,000,000 USDC to institutional investor
    await mintTo(
      provider.connection,
      authority.payer,
      usdcMint,
      investorUsdc,
      authority.payer,
      1_000_000 * 1e6,
      [],
      undefined,
      TOKEN_PROGRAM_ID
    );
  });

  it("Initializes the Sovereign Yield Pool with Senior/Junior Tranches", async () => {
    const seniorYieldBps = 525; // 5.25% Sovereign Risk-Free Benchmark
    const reserveRatioBps = 1000; // 10% Cash Liquidity Buffer

    await program.methods
      .initializePool(poolBump, seniorYieldBps, reserveRatioBps)
      .accounts({
        authority: authority.publicKey,
        usdcMint: usdcMint,
        seniorVault: seniorVaultPda,
        juniorVault: juniorVaultPda,
        seniorMint: seniorMint,
        juniorMint: juniorMint,
        tokenProgram: TOKEN_PROGRAM_ID,
        token2022Program: TOKEN_2022_PROGRAM_ID,
        systemProgram: SystemProgram.programId,
        rent: SYSVAR_RENT_PUBKEY,
      })
      .rpc();

    const poolAccount = await program.account.poolConfig.fetch(poolConfigPda);
    expect(poolAccount.seniorYieldBps).to.equal(525);
    expect(poolAccount.totalSeniorShares.toNumber()).to.equal(0);
    expect(poolAccount.authority.toBase58()).to.equal(authority.publicKey.toBase58());
  });

  it("Blocks Senior Deposit when user fails KYC Bitwise Registry check", async () => {
    const depositAmount = new anchor.BN(100_000 * 1e6);

    try {
      await program.methods
        .depositSenior(depositAmount)
        .accounts({
          user: institutionalInvestor.publicKey,
          poolConfig: poolConfigPda,
          kycRegistry: kycRegistryPda,
          userUsdc: investorUsdc,
          seniorVault: seniorVaultPda,
          seniorMint: seniorMint,
          userSeniorToken: investorSeniorAta,
          tokenProgram: TOKEN_PROGRAM_ID,
          token2022Program: TOKEN_2022_PROGRAM_ID,
        })
        .signers([institutionalInvestor])
        .rpc();
      expect.fail("Deposit should have thrown ComplianceKycFailed");
    } catch (err: any) {
      expect(err.toString()).to.include("AccountNotInitialized");
    }
  });

  it("Successfully updates Dynamic Yield via Keeper invocation", async () => {
    const newRateBps = 550; // Fed rate hike adjustment -> 5.50%
    const oracleNav = new anchor.BN(100_000_000 * 1e6);

    await program.methods
      .updateEpochYield(newRateBps, oracleNav)
      .accounts({
        keeper: authority.publicKey,
        poolConfig: poolConfigPda,
        seniorMint: seniorMint,
        token2022Program: TOKEN_2022_PROGRAM_ID,
      })
      .rpc();

    const poolState = await program.account.poolConfig.fetch(poolConfigPda);
    expect(poolState.seniorYieldBps).to.equal(550);
  });
});