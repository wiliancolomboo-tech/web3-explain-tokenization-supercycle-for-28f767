use anchor_lang::prelude::*;
use anchor_spl::token_interface::{self, Mint, TokenAccount, TokenInterface, TransferChecked};
use crate::state::{PoolConfig, LoanCovenant};
use crate::error::CreditPoolError;

#[derive(Accounts)]
pub struct ProcessRepayment<'info> {
    #[account(mut)]
    pub borrower: Signer<'info>,

    #[account(
        mut,
        seeds = [b"pool_config", pool_config.funding_mint.as_ref(), pool_config.authority.as_ref()],
        bump = pool_config.bump,
        has_one = capital_vault,
    )]
    pub pool_config: Account<'info, PoolConfig>,

    #[account(
        mut,
        seeds = [b"loan_covenant", pool_config.key().as_ref(), borrower.key().as_ref()],
        bump = loan_covenant.bump,
        has_one = borrower,
    )]
    pub loan_covenant: Account<'info, LoanCovenant>,

    #[account(
        mut,
        token::mint = pool_config.funding_mint,
        token::authority = borrower,
    )]
    pub borrower_token_account: InterfaceAccount<'info, TokenAccount>,

    #[account(mut)]
    pub capital_vault: InterfaceAccount<'info, TokenAccount>,

    pub funding_mint: InterfaceAccount<'info, Mint>,
    pub token_program: Interface<'info, TokenInterface>,
}

pub fn handler(
    ctx: Context<ProcessRepayment>,
    principal_amount: u64,
    interest_amount: u64,
) -> Result<()> {
    let pool = &mut ctx.accounts.pool_config;
    let covenant = &mut ctx.accounts.loan_covenant;
    let total_payment = principal_amount
        .checked_add(interest_amount)
        .ok_or(CreditPoolError::MathOverflow)?;

    // Atomic Transfer of repayment funds from borrower to capital vault
    let transfer_ctx = CpiContext::new(
        ctx.accounts.token_program.to_account_info(),
        TransferChecked {
            from: ctx.accounts.borrower_token_account.to_account_info(),
            mint: ctx.accounts.funding_mint.to_account_info(),
            to: ctx.accounts.capital_vault.to_account_info(),
            authority: ctx.accounts.borrower.to_account_info(),
        },
    );
    token_interface::transfer_checked(transfer_ctx, total_payment, ctx.accounts.funding_mint.decimals)?;

    // Watermark accounting updates
    if principal_amount > 0 {
        pool.outstanding_debt = pool
            .outstanding_debt
            .saturating_sub(principal_amount);
        covenant.principal_repaid = covenant
            .principal_repaid
            .checked_add(principal_amount)
            .ok_or(CreditPoolError::MathOverflow)?;
    }

    pool.accumulated_interest = pool
        .accumulated_interest
        .checked_add(interest_amount)
        .ok_or(CreditPoolError::MathOverflow)?;
    covenant.interest_repaid = covenant
        .interest_repaid
        .checked_add(interest_amount)
        .ok_or(CreditPoolError::MathOverflow)?;

    let clock = Clock::get()?;
    emit!(RepaymentProcessedEvent {
        borrower: ctx.accounts.borrower.key(),
        principal_repaid: principal_amount,
        interest_repaid: interest_amount,
        remaining_debt: pool.outstanding_debt,
        timestamp: clock.unix_timestamp,
    });

    Ok(())
}

#[event]
pub struct RepaymentProcessedEvent {
    pub borrower: Pubkey,
    pub principal_repaid: u64,
    pub interest_repaid: u64,
    pub remaining_debt: u64,
    pub timestamp: i64,
}