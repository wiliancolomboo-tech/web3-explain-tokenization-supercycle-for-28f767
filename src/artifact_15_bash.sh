# Build Anchor binary artifacts
anchor build

# Deploy Program IDs to Devnet
solana program deploy ./target/deploy/invoice_credit_engine.so --network devnet
solana program deploy ./target/deploy/compliance_transfer_hook.so --network devnet