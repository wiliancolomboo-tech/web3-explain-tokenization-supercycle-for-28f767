# Set Solana CLI target
solana config set --url https://api.mainnet-beta.solana.com

# Deploy Program Buffer & Upgrade
anchor build --verifiable
solana program deploy target/deploy/sol_rwa_credit.so \
  --program-id Credit1111111111111111111111111111111111111 \
  --keypair /path/to/deployer-authority.json