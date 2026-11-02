use anchor_lang::prelude::*;

pub mod errors;
pub mod state;
pub mod instructions;

use instructions::initialize_pool::*;
use instructions::register_investor::*;
use instructions::deposit_capital::*;
use instructions::process_repayment::*;
use instructions::redeem_yield::*;

declare_id!("CRed1tVauLt111111111111111111111111111111111");

#[program]
pub mod sol_rwa_credit {
    use super::*;

    pub fn initialize_pool(
        ctx: Context<InitializePool>,
        pool_id: [u8; 16],
        senior_target: u64,
        junior_target: u64,
        senior_apr_bps: u32,
        junior_apr_bps: u32,
        duration_seconds: i64,
        legal_contract_hash: [u8; 32],
    ) -> Result<()> {
        instructions::initialize_pool::handler(
            ctx,
            pool_id,
            senior_target,
            junior_target,
            senior_apr_bps,
            junior_apr_bps,
            duration_seconds,
            legal_contract_hash,
        )
    }

    pub fn register_investor(
        ctx: Context<RegisterInvestor>,
        is_accredited: bool,
        validity_duration_seconds: i64,
        jurisdiction: [u8; 3],
    ) -> Result<()> {
        instructions::register_investor::handler(ctx, is_accredited, validity_duration_seconds, jurisdiction)
    }

    pub fn deposit_capital(ctx: Context<DepositCapital>, amount: u64, is_senior: bool) -> Result<()> {
        instructions::deposit_capital::handler(ctx, amount, is_senior)
    }

    pub fn process_repayment(ctx: Context<ProcessRepayment>, principal_amount: u64, interest_amount: u64) -> Result<()> {
        instructions::process_repayment::handler(ctx, principal_amount, interest_amount)
    }

    pub fn redeem_yield(ctx: Context<RedeemYield>, tokens_to_redeem: u64) -> Result<()> {
        instructions::redeem_yield::handler(ctx, tokens_to_redeem)
    }
}