# 1. Clone & Build Solana Anchor Program
git clone https://github.com/sponsor-org/solana-clean-energy-rwa.git
cd solana-clean-energy-rwa
anchor build

# 2. Deploy to Solana Devnet
solana config set --url https://api.devnet.solana.com
solana airdrop 2
anchor deploy --program-name clean_energy_rwa

# 3. Configure Orchestration Client
cd client
cp .env.example .env
npm install
npm run build

# 4. Trigger Simulated Off-taker Yield Disbursement
npm run start deposit-yield 250000