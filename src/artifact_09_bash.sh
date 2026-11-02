# 1. Build and verify the Anchor program
anchor build

# 2. Deploy to Solana Devnet / Mainnet-Beta
solana program deploy \
  --program-id target/deploy/solana_rwa_credit-keypair.json \
  target/deploy/solana_rwa_credit.so \
  --url devnet

# 3. Spin up the enterprise automation worker
docker-compose up --build -d

# 4. View real-time webhook & event logs
docker-compose logs -f