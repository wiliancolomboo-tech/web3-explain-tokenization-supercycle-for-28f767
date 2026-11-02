use anchor_lang::prelude::*;
use anchor_spl::token_2022::{self, Burn, MintTo, TransferChecked};
use anchor_spl::token_interface::{Mint, Token2022, TokenAccount};

declare_id!("InvCred1111111111111111111111111111111111111");

pub const VAULT_SEED: &[u8] = b"invoice_vault";
pub const ESCROW_SEED: &[u8] = b"escrow_vault";
pub const BPS_DIVISOR: u64 = 10_000;
pub const SECONDS_PER_YEAR: u64 = 31_536_000;

#[program]
pub mod invoice_credit_engine {
    use super::*;

    /// Initialize a new corporate invoice financing pool backed by real-world accounts receivable.
    pub fn initialize_pool(
        ctx: Context<InitializePool>,
        pool_id: u64,
        invoice_face_value: u64,
        financing_amount: u64,
        discount_rate_bps: u64, // e.g. 800 = 8.00% APR
        duration_seconds: i64,
        invoice_hash: [u8; 32],
    ) -> Result<()> {
        require!(financing_amount < invoice_face_value, CreditError::InvalidFinancingAmount);
        require!(discount_rate_bps <= 3000, CreditError::ExcessiveDiscountRate);
        require!(duration_seconds > 0, CreditError::InvalidDuration);

        let clock = Clock::get()?;
        let pool = &mut ctx.accounts.pool;

        pool.originator = ctx.accounts.originator.key();
        pool.stable_mint = ctx.accounts.stable_mint.key();
        pool.tranche_mint = ctx.accounts.tranche_mint.key();
        pool.pool_id = pool_id;
        pool.invoice_face_value = invoice_face_value;
        pool.financing_amount = financing_amount;
        pool.funded_amount = 0;
        pool.repaid_amount = 0;
        pool.discount_rate_bps = discount_rate_bps;
        pool.created_at = clock.unix_timestamp;
        pool.maturity_timestamp = clock.unix_timestamp.checked_add(duration_seconds).ok_or(CreditError::MathOverflow)?;
        pool.invoice_hash = invoice_hash;
        pool.is_settled = false;
        pool.is_defaulted = false;
        pool.bump = ctx.bumps.pool;
        pool.escrow_bump = ctx.bumps.pool_escrow;

        emit!(PoolInitializedEvent {
            pool_id,
            originator: pool.originator,
            financing_amount,
            invoice_face_value,
            maturity_timestamp: pool.maturity_timestamp,
        });

        Ok(())
    }

    /// Institutional LP deposits stablecoins (USDC) to fund invoice origination, receiving Senior Tranche Token-2022 notes.
    pub fn supply_liquidity(ctx: Context<SupplyLiquidity>, amount: u64) -> Result<()> {
        let pool = &mut ctx.accounts.pool;
        let clock = Clock::get()?;

        require!(!pool.is_settled, CreditError::PoolAlreadySettled);
        require!(!pool.is_defaulted, CreditError::PoolInDefault);
        require!(clock.unix_timestamp < pool.maturity_timestamp, CreditError::PoolMatured);
        
        let available_to_fund = pool.financing_amount.checked_sub(pool.funded_amount).ok_or(CreditError::MathOverflow)?;
        require!(amount > 0 && amount <= available_to_fund, CreditError::DepositExceedsCapacity);

        // 1. Transfer payment token (USDC) from LP to the originator directly to disburse liquidity
        token_2022::transfer_checked(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                TransferChecked {
                    from: ctx.accounts.investor_stable_account.to_account_info(),
                    mint: ctx.accounts.stable_mint.to_account_info(),
                    to: ctx.accounts.originator_stable_account.to_account_info(),
                    authority: ctx.accounts.investor.to_account_info(),
                },
            ),
            amount,
            ctx.accounts.stable_mint.decimals,
        )?;

        // 2. Mint Senior Tranche Token-2022 Notes to investor. Transfer Hook is natively active on these tokens.
        let pool_id_bytes = pool.pool_id.to_le_bytes();
        let seeds = &[
            VAULT_SEED,
            pool_id_bytes.as_ref(),
            &[pool.bump],
        ];
        let signer_seeds = &[&seeds[..]];

        token_2022::mint_to(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                MintTo {
                    mint: ctx.accounts.tranche_mint.to_account_info(),
                    to: ctx.accounts.investor_tranche_account.to_account_info(),
                    authority: pool.to_account_info(),
                },
                signer_seeds,
            ),
            amount,
        )?;

        pool.funded_amount = pool.funded_amount.checked_add(amount).ok_or(CreditError::MathOverflow)?;

        emit!(LiquiditySuppliedEvent {
            pool_id: pool.pool_id,
            investor: ctx.accounts.investor.key(),
            amount,
            total_funded: pool.funded_amount,
        });

        Ok(())
    }

    /// Corporate debtor or originator settles the full invoice value into the escrow account.
    pub fn settle_invoice(ctx: Context<SettleInvoice>, amount: u64) -> Result<()> {
        let pool = &mut ctx.accounts.pool;
        require!(!pool.is_settled, CreditError::PoolAlreadySettled);

        // Deposit settlement funds to Escrow Token Account
        token_2022::transfer_checked(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                TransferChecked {
                    from: ctx.accounts.payer_stable_account.to_account_info(),
                    mint: ctx.accounts.stable_mint.to_account_info(),
                    to: ctx.accounts.pool_escrow.to_account_info(),
                    authority: ctx.accounts.payer.to_account_info(),
                },
            ),
            amount,
            ctx.accounts.stable_mint.decimals,
        )?;

        pool.repaid_amount = pool.repaid_amount.checked_add(amount).ok_or(CreditError::MathOverflow)?;

        // Calculate expected repayment with interest
        let total_due = pool.calculate_total_expected_return()?;
        if pool.repaid_amount >= total_due {
            pool.is_settled = true;
        }

        emit!(InvoiceSettledEvent {
            pool_id: pool.pool_id,
            settler: ctx.accounts.payer.key(),
            amount,
            is_fully_settled: pool.is_settled,
        });

        Ok(())
    }

    /// Institutional LP burns Tranche tokens to withdraw principal plus accrued yield.
    pub fn redeem_yield(ctx: Context<RedeemYield>, tranche_tokens_to_burn: u64) -> Result<()> {
        let pool = &ctx.accounts.pool;
        require!(pool.is_settled, CreditError::PoolNotSettled);
        require!(tranche_tokens_to_burn > 0, CreditError::InvalidRedemptionAmount);

        // Calculate proportional redemption: (tranche_burn / total_funded) * repaid_amount
        let payout = (tranche_tokens_to_burn as u128)
            .checked_mul(pool.repaid_amount as u128)
            .ok_or(CreditError::MathOverflow)?
            .checked_div(pool.funded_amount as u128)
            .ok_or(CreditError::MathOverflow)? as u64;

        // 1. Burn tranche tokens from investor
        token_2022::burn(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                Burn {
                    mint: ctx.accounts.tranche_mint.to_account_info(),
                    from: ctx.accounts.investor_tranche_account.to_account_info(),
                    authority: ctx.accounts.investor.to_account_info(),
                },
            ),
            tranche_tokens_to_burn,
        )?;

        // 2. Transfer stablecoins from escrow PDA to investor
        let pool_id_bytes = pool.pool_id.to_le_bytes();
        let seeds = &[
            ESCROW_SEED,
            pool_id_bytes.as_ref(),
            &[pool.escrow_bump],
        ];
        let signer_seeds = &[&seeds[..]];

        token_2022::transfer_checked(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                TransferChecked {
                    from: ctx.accounts.pool_escrow.to_account_info(),
                    mint: ctx.accounts.stable_mint.to_account_info(),
                    to: ctx.accounts.investor_stable_account.to_account_info(),
                    authority: ctx.accounts.pool_escrow.to_account_info(),
                },
                signer_seeds,
            ),
            payout,
            ctx.accounts.stable_mint.decimals,
        )?;

        emit!(YieldRedeemedEvent {
            pool_id: pool.pool_id,
            investor: ctx.accounts.investor.key(),
            tokens_burned: tranche_tokens_to_burn,
            stable_payout: payout,
        });

        Ok(())
    }

    /// Mark pool as defaulted if maturity has elapsed without full settlement
    pub fn trigger_default(ctx: Context<TriggerDefault>) -> Result<()> {
        let pool = &mut ctx.accounts.pool;
        let clock = Clock::get()?;

        require!(!pool.is_settled, CreditError::PoolAlreadySettled);
        require!(clock.unix_timestamp > pool.maturity_timestamp, CreditError::PoolNotMatured);

        pool.is_defaulted = true;

        emit!(PoolDefaultedEvent {
            pool_id: pool.pool_id,
            originator: pool.originator,
            unpaid_balance: pool.calculate_total_expected_return()?.saturating_sub(pool.repaid_amount),
            timestamp: clock.unix_timestamp,
        });

        Ok(())
    }
}

// -----------------------------------------------------------------------------
// ACCOUNT CONTEXTS
// -----------------------------------------------------------------------------

#[derive(Accounts)]
#[instruction(pool_id: u64)]
pub struct InitializePool<'info> {
    #[account(
        init,
        payer = originator,
        space = 8 + InvoicePool::INIT_SPACE,
        seeds = [VAULT_SEED, pool_id.to_le_bytes().as_ref()],
        bump
    )]
    pub pool: Box<Account<'info, InvoicePool>>,

    #[account(mut)]
    pub originator: Signer<'info>,

    pub stable_mint: Box<InterfaceAccount<'info, Mint>>,

    #[account(
        init,
        payer = originator,
        mint::decimals = 6,
        mint::authority = pool,
        mint::freeze_authority = pool,
    )]
    pub tranche_mint: Box<InterfaceAccount<'info, Mint>>,

    #[account(
        init,
        payer = originator,
        seeds = [ESCROW_SEED, pool_id.to_le_bytes().as_ref()],
        bump,
        token::mint = stable_mint,
        token::authority = pool_escrow,
        token::token_program = token_program
    )]
    pub pool_escrow: Box<InterfaceAccount<'info, TokenAccount>>,

    pub system_program: Program<'info, System>,
    pub token_program: Interface<'info, Token2022>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
pub struct SupplyLiquidity<'info> {
    #[account(
        mut,
        seeds = [VAULT_SEED, pool.pool_id.to_le_bytes().as_ref()],
        bump = pool.bump
    )]
    pub pool: Box<Account<'info, InvoicePool>>,

    #[account(mut)]
    pub investor: Signer<'info>,

    #[account(
        mut,
        constraint = investor_stable_account.mint == pool.stable_mint @ CreditError::MintMismatch,
        constraint = investor_stable_account.owner == investor.key() @ CreditError::OwnerMismatch
    )]
    pub investor_stable_account: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        mut,
        constraint = originator_stable_account.mint == pool.stable_mint @ CreditError::MintMismatch,
        constraint = originator_stable_account.owner == pool.originator @ CreditError::OwnerMismatch
    )]
    pub originator_stable_account: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        mut,
        constraint = tranche_mint.key() == pool.tranche_mint @ CreditError::MintMismatch
    )]
    pub tranche_mint: Box<InterfaceAccount<'info, Mint>>,

    #[account(
        mut,
        constraint = investor_tranche_account.mint == pool.tranche_mint @ CreditError::MintMismatch,
        constraint = investor_tranche_account.owner == investor.key() @ CreditError::OwnerMismatch
    )]
    pub investor_tranche_account: Box<InterfaceAccount<'info, TokenAccount>>,

    pub stable_mint: Box<InterfaceAccount<'info, Mint>>,
    pub token_program: Interface<'info, Token2022>,
}

#[derive(Accounts)]
pub struct SettleInvoice<'info> {
    #[account(
        mut,
        seeds = [VAULT_SEED, pool.pool_id.to_le_bytes().as_ref()],
        bump = pool.bump
    )]
    pub pool: Box<Account<'info, InvoicePool>>,

    #[account(mut)]
    pub payer: Signer<'info>,

    #[account(
        mut,
        constraint = payer_stable_account.mint == pool.stable_mint @ CreditError::MintMismatch
    )]
    pub payer_stable_account: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        mut,
        seeds = [ESCROW_SEED, pool.pool_id.to_le_bytes().as_ref()],
        bump = pool.escrow_bump
    )]
    pub pool_escrow: Box<InterfaceAccount<'info, TokenAccount>>,

    pub stable_mint: Box<InterfaceAccount<'info, Mint>>,
    pub token_program: Interface<'info, Token2022>,
}

#[derive(Accounts)]
pub struct RedeemYield<'info> {
    #[account(
        seeds = [VAULT_SEED, pool.pool_id.to_le_bytes().as_ref()],
        bump = pool.bump
    )]
    pub pool: Box<Account<'info, InvoicePool>>,

    #[account(mut)]
    pub investor: Signer<'info>,

    #[account(
        mut,
        constraint = investor_tranche_account.mint == pool.tranche_mint @ CreditError::MintMismatch,
        constraint = investor_tranche_account.owner == investor.key() @ CreditError::OwnerMismatch
    )]
    pub investor_tranche_account: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        mut,
        constraint = tranche_mint.key() == pool.tranche_mint @ CreditError::MintMismatch
    )]
    pub tranche_mint: Box<InterfaceAccount<'info, Mint>>,

    #[account(
        mut,
        seeds = [ESCROW_SEED, pool.pool_id.to_le_bytes().as_ref()],
        bump = pool.escrow_bump
    )]
    pub pool_escrow: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        mut,
        constraint = investor_stable_account.mint == pool.stable_mint @ CreditError::MintMismatch,
        constraint = investor_stable_account.owner == investor.key() @ CreditError::OwnerMismatch
    )]
    pub investor_stable_account: Box<InterfaceAccount<'info, TokenAccount>>,

    pub stable_mint: Box<InterfaceAccount<'info, Mint>>,
    pub token_program: Interface<'info, Token2022>,
}

#[derive(Accounts)]
pub struct TriggerDefault<'info> {
    #[account(
        mut,
        seeds = [VAULT_SEED, pool.pool_id.to_le_bytes().as_ref()],
        bump = pool.bump
    )]
    pub pool: Box<Account<'info, InvoicePool>>,
    pub caller: Signer<'info>,
}

// -----------------------------------------------------------------------------
// STATE STRUCTS & LOGIC
// -----------------------------------------------------------------------------

#[account]
#[derive(InitSpace)]
pub struct InvoicePool {
    pub originator: Pubkey,
    pub stable_mint: Pubkey,
    pub tranche_mint: Pubkey,
    pub pool_id: u64,
    pub invoice_face_value: u64,
    pub financing_amount: u64,
    pub funded_amount: u64,
    pub repaid_amount: u64,
    pub discount_rate_bps: u64,
    pub created_at: i64,
    pub maturity_timestamp: i64,
    pub invoice_hash: [u8; 32],
    pub is_settled: bool,
    pub is_defaulted: bool,
    pub bump: u8,
    pub escrow_bump: u8,
}

impl InvoicePool {
    pub fn calculate_total_expected_return(&self) -> Result<u64> {
        let duration = self.maturity_timestamp.saturating_sub(self.created_at) as u64;
        let interest = (self.funded_amount as u128)
            .checked_mul(self.discount_rate_bps as u128)
            .ok_or(CreditError::MathOverflow)?
            .checked_mul(duration as u128)
            .ok_or(CreditError::MathOverflow)?
            .checked_div((BPS_DIVISOR as u128) * (SECONDS_PER_YEAR as u128))
            .ok_or(CreditError::MathOverflow)? as u64;

        self.funded_amount.checked_add(interest).ok_or(CreditError::MathOverflow.into())
    }
}

// -----------------------------------------------------------------------------
// EVENTS & ERRORS
// -----------------------------------------------------------------------------

#[event]
pub struct PoolInitializedEvent {
    pub pool_id: u64,
    pub originator: Pubkey,
    pub financing_amount: u64,
    pub invoice_face_value: u64,
    pub maturity_timestamp: i64,
}

#[event]
pub struct LiquiditySuppliedEvent {
    pub pool_id: u64,
    pub investor: Pubkey,
    pub amount: u64,
    pub total_funded: u64,
}

#[event]
pub struct InvoiceSettledEvent {
    pub pool_id: u64,
    pub settler: Pubkey,
    pub amount: u64,
    pub is_fully_settled: bool,
}

#[event]
pub struct YieldRedeemedEvent {
    pub pool_id: u64,
    pub investor: Pubkey,
    pub tokens_burned: u64,
    pub stable_payout: u64,
}

#[event]
pub struct PoolDefaultedEvent {
    pub pool_id: u64,
    pub originator: Pubkey,
    pub unpaid_balance: u64,
    pub timestamp: i64,
}

#[error_code]
pub enum CreditError {
    #[msg("Financing amount must be strictly less than the invoice face value.")]
    InvalidFinancingAmount,
    #[msg("Discount rate exceeds acceptable enterprise risk threshold (max 30%).")]
    ExcessiveDiscountRate,
    #[msg("Duration must be positive.")]
    InvalidDuration,
    #[msg("Numerical calculation triggered an arithmetic overflow/underflow.")]
    MathOverflow,
    #[msg("Pool has already achieved full repayment settlement.")]
    PoolAlreadySettled,
    #[msg("Invoice pool has matured; liquidity deposits are closed.")]
    PoolMatured,
    #[msg("Invoice pool has defaulted on obligations.")]
    PoolInDefault,
    #[msg("Deposit amount exceeds remaining financing requirement.")]
    DepositExceedsCapacity,
    #[msg("Pool has not yet achieved settlement status.")]
    PoolNotSettled,
    #[msg("Invalid redemption amount specified.")]
    InvalidRedemptionAmount,
    #[msg("Pool maturity date has not elapsed yet.")]
    PoolNotMatured,
    #[msg("Mismatched token mint constraint.")]
    MintMismatch,
    #[msg("Account owner mismatch.")]
    OwnerMismatch,
}