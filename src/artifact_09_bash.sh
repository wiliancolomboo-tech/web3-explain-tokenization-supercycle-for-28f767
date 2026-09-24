# Set Solana CLI to local validator
solana config set --url localhost

# Generate program authority keypair
solana-keygen new -o target/deploy/authority-keypair.json --no-bip39-passphrase --force