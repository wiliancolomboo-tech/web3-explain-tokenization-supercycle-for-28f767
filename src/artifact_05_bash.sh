# 1. Clone repository & install dependencies
git clone https://github.com/sponsor-org/solpv-tokenization-engine.git
cd solpv-tokenization-engine
yarn install

# 2. Build Anchor Rust Program
anchor build

# 3. Configure Solana CLI for target cluster (e.g., Devnet)
solana config set --url https://api.devnet.solana.com
solana airdrop 2

# 4. Deploy Program to Solana
anchor deploy --provider.cluster devnet

# 5. Run Integration Verification Test Suite
anchor test --skip-local-validator