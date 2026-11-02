# 1. Clone repository and build Solana program
git clone https://github.com/institutional-rwa/solana-supercycle-engine.git
cd solana-supercycle-engine
cargo build-sbf

# 2. Deploy Anchor Program to Solana Devnet
solana config set --url https://api.devnet.solana.com
solana airdrop 2
anchor deploy --provider.cluster devnet

# 3. Initialize Enterprise Environment Variables
cp .env.example .env
# Edit .env with your RESEND_API_KEY and SLACK_WEBHOOK_URL

# 4. Launch Containerized Enterprise Monitoring Stack
docker compose up -d --build