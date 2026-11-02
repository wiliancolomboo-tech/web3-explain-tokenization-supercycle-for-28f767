# 1. Install prerequisites
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
sh -c "$(curl -sSfL https://release.anza.xyz/stable/install)"
cargo install --git https://github.com/coral-xyz/anchor --tag v0.30.1 anchor-cli --locked

# 2. Build Anchor Artifacts
anchor build

# 3. Synchronize program keys
anchor keys sync

# 4. Deploy to Solana Devnet
solana config set --url https://api.devnet.solana.com
anchor deploy --provider.cluster devnet