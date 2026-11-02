# Cargo.toml dependencies required
[dependencies]
anchor-lang = { version = "0.30.1", features = ["init-if-needed"] }
anchor-spl = { version = "0.30.1", features = ["token_2022", "mint_close_authority"] }
spl-transfer-hook-interface = "0.6.3"
spl-tlv-account-resolution = "0.6.3"
spl-type-length-value = "0.4.3"
spl-pod = "0.2.2"
solana-program = "1.18.26"
thiserror = "1.0"