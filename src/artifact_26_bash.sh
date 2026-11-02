# 1. Install dependencies & configure Solana CLI
solana config set --url https://api.devnet.solana.com
solana-keygen new --outfile deployer-keypair.json --no-bip39-passphrase

# 2. Build program binary
anchor build

# 3. Synchronize program keys
solana address -k target/deploy/rwa_token_engine-keypair.json
# Replace declare_id!("...") in programs/rwa-token-engine/src/lib.rs

# 4. Deploy program to Solana Devnet / Mainnet
solana program deploy \
  --program-id target/deploy/rwa_token_engine-keypair.json \
  target/deploy/rwa_token_engine.so