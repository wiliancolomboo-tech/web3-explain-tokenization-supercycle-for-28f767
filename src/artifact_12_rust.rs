use anchor_lang::prelude::*;

pub mod error;
pub mod instructions;
pub mod state;

use instructions::deposit_liquidity::*;
use instructions::initialize_pool::*;
use instructions::process_repayment::*;
use instructions::register_compliance::*;
use instructions::transfer_hook_check::*;

declare_id!("CRDTVkZ6Z6g2cRnmrXy1p7zPj4tF4zM2XyqZ7rT4yU8w");

#[program]
pub mod sol_credit_core {
    use super::*;

    pub fn initialize_pool(
        ctx: Context<InitializePool>,
        senior_apr_bps: u64,
        senior_capacity: u64,
        loan_duration_seconds: i64,
    ) -> Result<()> {
        instructions::initialize_pool::handler(ctx, senior_apr_bps, senior_capacity, loan_duration_seconds)
    }

    pub fn register_compliance(
        ctx: Context<RegisterCompliance>,
        investor_wallet: Pubkey,
        kyc_expiry: i64,
        jurisdiction_flags: u32,
        is_sanctioned: bool,
    ) -> Result<()> {
        instructions::register_compliance::handler(
            ctx,
            investor_wallet,
            kyc_expiry,
            jurisdiction_flags,
            is_sanctioned,
        )
    }

    pub fn deposit_liquidity(ctx: Context<DepositLiquidity>, amount: u64) -> Result<()> {
        instructions::deposit_liquidity::handler(ctx, amount)
    }

    pub fn process_repayment(
        ctx: Context<ProcessRepayment>,
        principal_amount: u64,
        interest_amount: u64,
    ) -> Result<()> {
        instructions::process_repayment::handler(ctx, principal_amount, interest_amount)
    }

    #[interface(spl_transfer_hook_interface::execute)]
    pub fn transfer_hook(ctx: Context<ExecuteTransferHook>, amount: u64) -> Result<()> {
        instructions::transfer_hook_check::handler(ctx, amount)
    }
}