# Switch to devnet
solana config set --url devnet

# Airdrop devnet SOL for deployment fees
solana airdrop 2

# Deploy program
anchor deploy --provider.cluster devnet

# Initialize pool via Anchor CLI
yarn run ts-node scripts/initialize-pool-cli.ts \
  --cluster devnet \
  --seniorYieldBps 525 \
  --reserveRatio 1000