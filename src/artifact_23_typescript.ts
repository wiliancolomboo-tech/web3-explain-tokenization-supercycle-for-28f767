import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { Keypair, PublicKey, SystemProgram } from "@solana/web3.js";
import {
  TOKEN_2022_PROGRAM_ID,
  createMint,
  createAccount,
  mintTo,
} from "@solana/spl-token";
import { assert, expect } from "chai";
import { RwaTokenEngine } from "../target/types/rwa_token_engine";

describe("Solana Token-2022 RWA Supercycle Integration Engine", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  const program = anchor.workspace.RwaTokenEngine as Program<RwaTokenEngine>;

  const authority = Keypair.generate();
  const complianceAuthority = Keypair.generate();
  const accreditedInvestor = Keypair.generate();
  const unaccreditedInvestor = Keypair.generate();
  const assetMint = Keypair.generate();
  const treasuryVault = Keypair.generate();

  let assetConfigPDA: PublicKey;
  let accreditedIdentityPDA: PublicKey;
  let unaccreditedIdentityPDA: PublicKey;

  before(async () => {
    // Airdrop SOL to test actors
    const airdropSig1 = await provider.connection.requestAirdrop(authority.publicKey, 10 * anchor.web3.LAMPORTS_PER_SOL);
    const airdropSig2 = await provider.connection.requestAirdrop(complianceAuthority.publicKey, 5 * anchor.web3.LAMPORTS_PER_SOL);
    await provider.connection.confirmTransaction(airdropSig1);
    await provider.connection.confirmTransaction(airdropSig2);

    [assetConfigPDA] = PublicKey.findProgramAddressSync(
      [Buffer.from("asset_config"), assetMint.publicKey.toBuffer()],
      program.programId
    );

    [accreditedIdentityPDA] = PublicKey.findProgramAddressSync(
      [Buffer.from("investor_identity"), accreditedInvestor.publicKey.toBuffer()],
      program.programId
    );

    [unaccreditedIdentityPDA] = PublicKey.findProgramAddressSync(
      [Buffer.from("investor_identity"), unaccreditedInvestor.publicKey.toBuffer()],
      program.programId
    );
  });

  it("1. Initializes Asset Configuration with 8.50% Yield & QIB Requirements", async () => {
    const isinArray = Array.from(Buffer.from("US9999SOLA01")); // 12 bytes
    const basisPoints = 850; // 8.50%
    const minTier = 2; // Qualified Institutional Buyer (QIB)

    await program.methods
      .initializeAsset(isinArray, basisPoints, minTier)
      .accounts({
        assetConfig: assetConfigPDA,
        assetMint: assetMint.publicKey,
        complianceAuthority: complianceAuthority.publicKey,
        treasuryVault: treasuryVault.publicKey,
        authority: authority.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .signers([authority, assetMint])
      .rpc();

    const config = await program.account.assetConfig.fetch(assetConfigPDA);
    expect(config.basisPointsRate).to.equal(850);
    expect(config.minimumInvestorTier).to.equal(2);
    expect(config.isPaused).to.be.false;
  });

  it("2. Registers Accredited QIB Investor (Tier 2 - Authorized)", async () => {
    const now = Math.floor(Date.now() / 1000);
    const validUntil = new anchor.BN(now + 365 * 86400); // 1 year validity
    const countryCode = Array.from(Buffer.from("US"));
    const sanctionsHash = new Array(32).fill(7);

    await program.methods
      .registerInvestor(2, countryCode, validUntil, sanctionsHash)
      .accounts({
        investorIdentity: accreditedIdentityPDA,
        assetConfig: assetConfigPDA,
        investorOwner: accreditedInvestor.publicKey,
        complianceAuthority: complianceAuthority.publicKey,
        payer: complianceAuthority.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .signers([complianceAuthority])
      .rpc();

    const identity = await program.account.investorIdentity.fetch(accreditedIdentityPDA);
    expect(identity.investorTier).to.equal(2);
    expect(identity.status).to.deep.equal({ approved: {} });
  });

  it("3. Registers Retail Investor (Tier 1 - Under-qualified for this tranche)", async () => {
    const now = Math.floor(Date.now() / 1000);
    const validUntil = new anchor.BN(now + 365 * 86400);
    const countryCode = Array.from(Buffer.from("GB"));
    const sanctionsHash = new Array(32).fill(1);

    await program.methods
      .registerInvestor(1, countryCode, validUntil, sanctionsHash)
      .accounts({
        investorIdentity: unaccreditedIdentityPDA,
        assetConfig: assetConfigPDA,
        investorOwner: unaccreditedInvestor.publicKey,
        complianceAuthority: complianceAuthority.publicKey,
        payer: complianceAuthority.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .signers([complianceAuthority])
      .rpc();

    const identity = await program.account.investorIdentity.fetch(unaccreditedIdentityPDA);
    expect(identity.investorTier).to.equal(1);
  });

  it("4. Blocks transfer to unaccredited investor via Transfer Hook evaluation", async () => {
    try {
      await program.methods
        .executeTransferHook(new anchor.BN(100_000_000))
        .accounts({
          assetConfig: assetConfigPDA,
          mint: assetMint.publicKey,
          sourceOwner: accreditedInvestor.publicKey,
          destinationOwner: unaccreditedInvestor.publicKey,
          sourceIdentity: accreditedIdentityPDA,
          destIdentity: unaccreditedIdentityPDA,
        })
        .rpc();
      assert.fail("Transfer should have been blocked due to insufficient investor tier");
    } catch (err: any) {
      expect(err.error.errorCode.code).to.equal("UnauthorizedInvestorTier");
    }
  });

  it("5. Rejects compliance registration with unauthorized authority signature", async () => {
    const maliciousActor = Keypair.generate();
    const fakeAirdrop = await provider.connection.requestAirdrop(maliciousActor.publicKey, 1000000000);
    await provider.connection.confirmTransaction(fakeAirdrop);

    const [fakePDA] = PublicKey.findProgramAddressSync(
      [Buffer.from("investor_identity"), maliciousActor.publicKey.toBuffer()],
      program.programId
    );

    try {
      await program.methods
        .registerInvestor(
          2,
          Array.from(Buffer.from("US")),
          new anchor.BN(Date.now() + 10000),
          new Array(32).fill(0)
        )
        .accounts({
          investorIdentity: fakePDA,
          assetConfig: assetConfigPDA,
          investorOwner: maliciousActor.publicKey,
          complianceAuthority: maliciousActor.publicKey, // Malicious authority key
          payer: maliciousActor.publicKey,
          systemProgram: SystemProgram.programId,
        })
        .signers([maliciousActor])
        .rpc();
      assert.fail("Should have failed with InvalidAuthority constraint violation");
    } catch (err: any) {
      expect(err.error.errorCode.code).to.equal("InvalidAuthority");
    }
  });
});