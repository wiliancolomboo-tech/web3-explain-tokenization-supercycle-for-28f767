use anchor_lang::prelude::*;
use anchor_spl::token_interface::{self, Mint, TokenAccount, TokenInterface, TransferChecked, MintTo};
use crate::state::{PoolConfig, ComplianceRecord};
use crate::error::CreditPoolError;

#[derive(Accounts)]
pub struct DepositLiquidity<'info> {
    #[account(mut)]
    pub investor: Signer<'info>,

    #[account(
        mut,
        seeds = [b"pool_config", pool_config.funding_mint.as_ref(), pool_config.authority.as_ref()],
        bump = pool_config.bump,
        has_one = capital_vault,
        has_one = senior_mint,
    )]
    pub pool_config: Account<'info, PoolConfig>,

    #[account(
        seeds = [b"compliance_record", investor.key().as_ref()],
        bump = compliance_record.bump,
    )]
    pub compliance_record: Account<'info, ComplianceRecord>,

    #[account(
        mut,
        token::mint = pool_config.funding_mint,
        token::authority = investor,
    )]
    pub investor_funding_account: InterfaceAccount<'info, TokenAccount>,

    #[account(mut)]
    pub capital_vault: InterfaceAccount<'info, TokenAccount>,

    #[account(mut)]
    pub senior_mint: InterfaceAccount<'info, Mint>,

    #[account(
        mut,
        token::mint = senior_mint,
        token::authority = investor,
    )]
    pub investor_senior_note_account: InterfaceAccount<'info, TokenAccount>,

    pub funding_mint: InterfaceAccount<'info, Mint>,
    pub token_program: Interface<'info, TokenInterface>,
    pub token_2022_program: Interface<'info, TokenInterface>,
}

pub fn handler(ctx: Context<DepositLiquidity>, amount: u64) -> Result<()> {
    let clock = Clock::get()?;
    let pool = &mut ctx.accounts.pool_config;
    let compliance = &ctx.accounts.compliance_record;

    // Enforce compliance assertions
    require!(pool.is_active, CreditPoolError::PoolInactive);
    require!(!compliance.is_sanctioned, CreditPoolError::JurisdictionRestricted);
    require!(compliance.kyc_expiry > clock.unix_timestamp, CreditPoolError::KycExpired);

    // Enforce tranche capacity
    let new_senior_total = pool
        .senior_deposited
        .checked_add(amount)
        .ok_or(CreditPoolError::MathOverflow)?;
    require!(new_senior_total <= pool.senior_capacity, CreditPoolError::SeniorTrancheFull);

    // Transfer stablecoin from Investor to Protocol Capital Vault
    let transfer_ctx = CpiContext::new(
        ctx.accounts.token_program.to_account_info(),
        TransferChecked {
            from: ctx.accounts.investor_funding_account.to_account_info(),
            mint: ctx.accounts.funding_mint.to_account_info(),
            to: ctx.accounts.capital_vault.to_account_info(),
            authority: ctx.accounts.investor.to_account_info(),
        },
    );
    token_interface::transfer_checked(transfer_ctx, amount, ctx.accounts.funding_mint.decimals)?;

    // Mint Senior LP Token-2022 Notes (1:1 par exchange during capital formation)
    let pool_authority = pool.authority;
    let funding_mint_key = pool.funding_mint;
    let signer_seeds: &[&[&[u8]]] = &[&[
        b"pool_config",
        funding_mint_key.as_ref(),
        pool_authority.as_ref(),
        &[pool.bump],
    ]];

    let mint_to_ctx = CpiContext::new_with_signer(
        ctx.accounts.token_2022_program.to_account_info(),
        MintTo {
            mint: ctx.accounts.senior_mint.to_account_info(),
            to: ctx.accounts.investor_senior_note_account.to_account_info(),
            authority: pool.to_account_info(),
        },
        signer_seeds,
    );
    token_interface::mint_to(mint_to_ctx, amount)?;

    pool.senior_deposited = new_senior_total;

    emit!(LiquidityDepositedEvent {
        investor: ctx.accounts.investor.key(),
        amount,
        new_total_pool_capital: pool.senior_deposited,
        timestamp: clock.unix_timestamp,
    });

    Ok(())
}

#[event]
pub struct LiquidityDepositedEvent {
    pub investor: Pubkey,
    pub amount: u64,
    pub new_total_pool_capital: u64,
    pub timestamp: i64,
}