use anchor_lang::prelude::*;

pub mod error;
pub mod instructions;
pub mod state;

use instructions::*;

declare_id!("RWA1111111111111111111111111111111111111111");

#[program]
pub mod rwa_token_engine {
    use super::*;

    pub fn initialize_asset(
        ctx: Context<InitializeAsset>,
        isin: [u8; 12],
        basis_points: u16,
        min_tier: u8,
    ) -> Result<()> {
        instructions::initialize_asset::handler(ctx, isin, basis_points, min_tier)
    }

    pub fn register_investor(
        ctx: Context<RegisterInvestor>,
        tier: u8,
        country_code: [u8; 2],
        valid_until: i64,
        sanctions_hash: [u8; 32],
    ) -> Result<()> {
        instructions::register_investor::handler(ctx, tier, country_code, valid_until, sanctions_hash)
    }

    pub fn execute_transfer_hook(ctx: Context<ExecuteTransferHook>, amount: u64) -> Result<()> {
        instructions::execute_transfer_hook::handler(ctx, amount)
    }

    pub fn service_yield(ctx: Context<ServiceYield>, payment_amount: u64) -> Result<()> {
        instructions::service_yield::handler(ctx, payment_amount)
    }
}