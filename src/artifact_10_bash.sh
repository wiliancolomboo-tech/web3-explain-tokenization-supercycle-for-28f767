#!/usr/bin/env bash
set -euo pipefail

echo "=========================================================="
echo "  Deploying Solana RWA Credit Supercycle Protocol         "
echo "=========================================================="

NETWORK="${1:-devnet}"
ANCHOR_WALLET="${ANCHOR_WALLET:-$HOME/.config/solana/id.json}"

echo ">> Network Target: $NETWORK"
echo ">> Deployer Authority: $ANCHOR_WALLET"

# Build Smart Contracts
anchor build

# Deploy Program
echo ">> Deploying Anchor Program to $NETWORK..."
anchor deploy --provider.cluster "$NETWORK" --provider.wallet "$ANCHOR_WALLET"

# Sync IDL & Metadata
echo ">> Uploading IDL and Initializing Token-2022 Transfer Hook configurations..."
anchor idl init --filepath target/idl/rwa_credit_supercycle.json $(solana address -k target/deploy/rwa_credit_supercycle-keypair.json) --provider.cluster "$NETWORK"

echo "=========================================================="
echo "  Deployment Complete. System Ready for Institutional Sync"
echo "=========================================================="