// File: programs/sol_rwa_credit/src/lib.rs

use anchor_lang::prelude::*;
use anchor_spl::token_interface::{self, Mint, TokenAccount, TokenInterface, TransferChecked};

declare_id!("Credit1111111111111111111111111111111111111");

pub const SENIOR_TRANCHE_YIELD_BPS: u64 = 850; // 8.50% APR
pub const BPS_DENOMINATOR: u64 = 10_000;
pub const SECONDS_PER_YEAR: u64 = 31_536_000;

#[program]
pub mod sol_rwa_credit {
    use super::*;

    pub fn initialize_pool(
        ctx: Context<InitializePool>,
        pool_id: String,
        principal_target: u64,
        tenor_seconds: i64,
        senior_ratio_bps: u64,
    ) -> Result<()> {
        require!(pool_id.len() <= 32, CreditError::IdentifierTooLong);
        require!(senior_ratio_bps <= BPS_DENOMINATOR, CreditError::InvalidRatio);
        require!(principal_target > 0, CreditError::ZeroAmount);
        require!(tenor_seconds > 0, CreditError::InvalidTenor);

        let pool = &mut ctx.accounts.pool;
        pool.authority = ctx.accounts.authority.key();
        pool.usdc_mint = ctx.accounts.usdc_mint.key();
        pool.senior_mint = ctx.accounts.senior_mint.key();
        pool.junior_mint = ctx.accounts.junior_mint.key();
        pool.vault = ctx.accounts.pool_vault.key();
        pool.pool_id = pool_id;
        pool.principal_target = principal_target;
        pool.total_funded = 0;
        pool.tenor_seconds = tenor_seconds;
        pool.senior_ratio_bps = senior_ratio_bps;
        pool.start_timestamp = 0;
        pool.maturity_timestamp = 0;
        pool.is_funded = false;
        pool.is_liquidated = false;
        pool.repaid_amount = 0;
        pool.bump = ctx.bumps.pool;

        emit!(PoolInitialized {
            pool_id: pool.pool_id.clone(),
            authority: pool.authority,
            principal_target,
            senior_ratio_bps,
        });

        Ok(())
    }

    pub fn register_investor(
        ctx: Context<RegisterInvestor>,
        investor_address: Pubkey,
        accredited_until: i64,
        kyc_hash: [u8; 32],
    ) -> Result<()> {
        let clock = Clock::get()?;
        require!(accredited_until > clock.unix_timestamp, CreditError::ExpiredKyc);

        let record = &mut ctx.accounts.investor_record;
        record.investor = investor_address;
        record.accredited_until = accredited_until;
        record.is_blacklisted = false;
        record.kyc_hash = kyc_hash;
        record.bump = ctx.bumps.investor_record;

        emit!(InvestorRegistered {
            investor: investor_address,
            accredited_until,
        });

        Ok(())
    }

    pub fn deposit_and_mint(
        ctx: Context<DepositAndMint>,
        amount: u64,
        is_senior: bool,
    ) -> Result<()> {
        require!(amount > 0, CreditError::ZeroAmount);
        let clock = Clock::get()?;
        let pool = &mut ctx.accounts.pool;
        let investor_record = &ctx.accounts.investor_record;

        require!(!pool.is_funded, CreditError::PoolAlreadyFunded);
        require!(!investor_record.is_blacklisted, CreditError::BlacklistedAccount);
        require!(
            investor_record.accredited_until > clock.unix_timestamp,
            CreditError::ExpiredKyc
        );

        // Enforce tranche target capacity
        let senior_target = (pool.principal_target as u128 * pool.senior_ratio_bps as u128 / BPS_DENOMINATOR as u128) as u64;
        let junior_target = pool.principal_target.checked_sub(senior_target).ok_or(CreditError::MathUnderflow)?;

        if is_senior {
            let current_senior_supply = ctx.accounts.senior_mint.supply;
            require!(
                current_senior_supply.checked_add(amount).ok_or(CreditError::MathOverflow)? <= senior_target,
                CreditError::TrancheCapacityExceeded
            );
        } else {
            let current_junior_supply = ctx.accounts.junior_mint.supply;
            require!(
                current_junior_supply.checked_add(amount).ok_or(CreditError::MathOverflow)? <= junior_target,
                CreditError::TrancheCapacityExceeded
            );
        }

        // Deposit USDC from investor to pool vault
        let cpi_accounts = TransferChecked {
            from: ctx.accounts.investor_usdc.to_account_info(),
            mint: ctx.accounts.usdc_mint.to_account_info(),
            to: ctx.accounts.pool_vault.to_account_info(),
            authority: ctx.accounts.investor.to_account_info(),
        };
        let cpi_program = ctx.accounts.token_program.to_account_info();
        let cpi_ctx = CpiContext::new(cpi_program, cpi_accounts);
        token_interface::transfer_checked(cpi_ctx, amount, ctx.accounts.usdc_mint.decimals)?;

        // Mint tranche token back to investor
        let pool_id_str = pool.pool_id.clone();
        let seeds = &[
            b"pool".as_ref(),
            pool_id_str.as_bytes(),
            &[pool.bump],
        ];
        let signer = &[&seeds[..]];

        if is_senior {
            let cpi_mint = token_interface::MintTo {
                mint: ctx.accounts.senior_mint.to_account_info(),
                to: ctx.accounts.investor_tranche_token.to_account_info(),
                authority: ctx.accounts.pool.to_account_info(),
            };
            token_interface::mint_to(
                CpiContext::new_with_signer(ctx.accounts.token_program.to_account_info(), cpi_mint, signer),
                amount,
            )?;
        } else {
            let cpi_mint = token_interface::MintTo {
                mint: ctx.accounts.junior_mint.to_account_info(),
                to: ctx.accounts.investor_tranche_token.to_account_info(),
                authority: ctx.accounts.pool.to_account_info(),
            };
            token_interface::mint_to(
                CpiContext::new_with_signer(ctx.accounts.token_program.to_account_info(), cpi_mint, signer),
                amount,
            )?;
        }

        pool.total_funded = pool.total_funded.checked_add(amount).ok_or(CreditError::MathOverflow)?;

        if pool.total_funded == pool.principal_target {
            pool.is_funded = true;
            pool.start_timestamp = clock.unix_timestamp;
            pool.maturity_timestamp = clock.unix_timestamp.checked_add(pool.tenor_seconds).ok_or(CreditError::MathOverflow)?;
            emit!(PoolFullyFunded {
                pool_id: pool.pool_id.clone(),
                start_timestamp: pool.start_timestamp,
                maturity_timestamp: pool.maturity_timestamp,
            });
        }

        emit!(FundsInvested {
            pool_id: pool.pool_id.clone(),
            investor: ctx.accounts.investor.key(),
            amount,
            is_senior,
        });

        Ok(())
    }

    pub fn process_repayment_waterfall(
        ctx: Context<ProcessRepaymentWaterfall>,
        payment_amount: u64,
    ) -> Result<()> {
        let clock = Clock::get()?;
        let pool = &mut ctx.accounts.pool;
        require!(pool.is_funded, CreditError::PoolNotFunded);
        require!(payment_amount > 0, CreditError::ZeroAmount);

        // Transfer funds from Originator / Servicer to the Pool Vault
        let cpi_accounts = TransferChecked {
            from: ctx.accounts.servicer_usdc.to_account_info(),
            mint: ctx.accounts.usdc_mint.to_account_info(),
            to: ctx.accounts.pool_vault.to_account_info(),
            authority: ctx.accounts.servicer.to_account_info(),
        };
        token_interface::transfer_checked(
            CpiContext::new(ctx.accounts.token_program.to_account_info(), cpi_accounts),
            payment_amount,
            ctx.accounts.usdc_mint.decimals,
        )?;

        // Waterfall Math: Senior gets interest + principal first
        let elapsed = (clock.unix_timestamp - pool.start_timestamp).max(0) as u64;
        let senior_supply = ctx.accounts.senior_mint.supply;
        
        // senior interest = senior_supply * SENIOR_TRANCHE_YIELD_BPS * elapsed / (SECONDS_PER_YEAR * BPS_DENOMINATOR)
        let senior_interest = (senior_supply as u128)
            .checked_mul(SENIOR_TRANCHE_YIELD_BPS as u128).unwrap()
            .checked_mul(elapsed as u128).unwrap()
            .checked_div((SECONDS_PER_YEAR as u128) * (BPS_DENOMINATOR as u128))
            .unwrap() as u64;

        let senior_obligations = senior_supply.checked_add(senior_interest).ok_or(CreditError::MathOverflow)?;
        
        let senior_payout = std::cmp::min(payment_amount, senior_obligations);
        let junior_payout = payment_amount.saturating_sub(senior_payout);

        pool.repaid_amount = pool.repaid_amount.checked_add(payment_amount).ok_or(CreditError::MathOverflow)?;

        emit!(RepaymentProcessed {
            pool_id: pool.pool_id.clone(),
            total_repayment: payment_amount,
            senior_portion: senior_payout,
            junior_portion: junior_payout,
            timestamp: clock.unix_timestamp,
        });

        Ok(())
    }
}

#[derive(Accounts)]
#[instruction(pool_id: String)]
pub struct InitializePool<'info> {
    #[account(
        init,
        payer = authority,
        space = 8 + CreditPool::LEN,
        seeds = [b"pool", pool_id.as_bytes()],
        bump
    )]
    pub pool: Box<Account<'info, CreditPool>>,

    #[account(mut)]
    pub authority: Signer<'info>,

    pub usdc_mint: Box<InterfaceAccount<'info, Mint>>,

    #[account(
        init,
        payer = authority,
        mint::decimals = 6,
        mint::authority = pool,
        seeds = [b"senior", pool_id.as_bytes()],
        bump
    )]
    pub senior_mint: Box<InterfaceAccount<'info, Mint>>,

    #[account(
        init,
        payer = authority,
        mint::decimals = 6,
        mint::authority = pool,
        seeds = [b"junior", pool_id.as_bytes()],
        bump
    )]
    pub junior_mint: Box<InterfaceAccount<'info, Mint>>,

    #[account(
        init,
        payer = authority,
        token::mint = usdc_mint,
        token::authority = pool,
        seeds = [b"vault", pool_id.as_bytes()],
        bump
    )]
    pub pool_vault: Box<InterfaceAccount<'info, TokenAccount>>,

    pub system_program: Program<'info, System>,
    pub token_program: Interface<'info, TokenInterface>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
pub struct RegisterInvestor<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,

    #[account(
        init_if_needed,
        payer = authority,
        space = 8 + InvestorRecord::LEN,
        seeds = [b"investor", investor_address.as_ref()],
        bump
    )]
    pub investor_record: Account<'info, InvestorRecord>,

    /// CHECK: Target investor to verify
    pub investor_address: AccountInfo<'info>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct DepositAndMint<'info> {
    #[account(mut)]
    pub investor: Signer<'info>,

    #[account(
        mut,
        seeds = [b"pool", pool.pool_id.as_bytes()],
        bump = pool.bump
    )]
    pub pool: Box<Account<'info, CreditPool>>,

    #[account(
        seeds = [b"investor", investor.key().as_ref()],
        bump = investor_record.bump,
        constraint = investor_record.investor == investor.key() @ CreditError::UnauthorizedInvestor
    )]
    pub investor_record: Account<'info, InvestorRecord>,

    pub usdc_mint: Box<InterfaceAccount<'info, Mint>>,

    #[account(
        mut,
        seeds = [b"senior", pool.pool_id.as_bytes()],
        bump
    )]
    pub senior_mint: Box<InterfaceAccount<'info, Mint>>,

    #[account(
        mut,
        seeds = [b"junior", pool.pool_id.as_bytes()],
        bump
    )]
    pub junior_mint: Box<InterfaceAccount<'info, Mint>>,

    #[account(
        mut,
        constraint = investor_usdc.mint == usdc_mint.key(),
        constraint = investor_usdc.owner == investor.key()
    )]
    pub investor_usdc: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        mut,
        seeds = [b"vault", pool.pool_id.as_bytes()],
        bump
    )]
    pub pool_vault: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        mut,
        constraint = investor_tranche_token.owner == investor.key()
    )]
    pub investor_tranche_token: Box<InterfaceAccount<'info, TokenAccount>>,

    pub token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct ProcessRepaymentWaterfall<'info> {
    #[account(mut)]
    pub servicer: Signer<'info>,

    #[account(
        mut,
        seeds = [b"pool", pool.pool_id.as_bytes()],
        bump = pool.bump,
        has_one = vault,
        has_one = senior_mint
    )]
    pub pool: Box<Account<'info, CreditPool>>,

    pub usdc_mint: Box<InterfaceAccount<'info, Mint>>,

    #[account(
        mut,
        seeds = [b"senior", pool.pool_id.as_bytes()],
        bump
    )]
    pub senior_mint: Box<InterfaceAccount<'info, Mint>>,

    #[account(mut)]
    pub pool_vault: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        mut,
        constraint = servicer_usdc.mint == usdc_mint.key(),
        constraint = servicer_usdc.owner == servicer.key()
    )]
    pub servicer_usdc: Box<InterfaceAccount<'info, TokenAccount>>,

    pub vault: Box<InterfaceAccount<'info, TokenAccount>>,
    pub token_program: Interface<'info, TokenInterface>,
}

#[account]
pub struct CreditPool {
    pub authority: Pubkey,
    pub usdc_mint: Pubkey,
    pub senior_mint: Pubkey,
    pub junior_mint: Pubkey,
    pub vault: Pubkey,
    pub pool_id: String,
    pub principal_target: u64,
    pub total_funded: u64,
    pub tenor_seconds: i64,
    pub senior_ratio_bps: u64,
    pub start_timestamp: i64,
    pub maturity_timestamp: i64,
    pub is_funded: bool,
    pub is_liquidated: bool,
    pub repaid_amount: u64,
    pub bump: u8,
}

impl CreditPool {
    pub const LEN: usize = 32 + 32 + 32 + 32 + 32 + (4 + 32) + 8 + 8 + 8 + 8 + 8 + 8 + 1 + 1 + 8 + 1;
}

#[account]
pub struct InvestorRecord {
    pub investor: Pubkey,
    pub accredited_until: i64,
    pub is_blacklisted: bool,
    pub kyc_hash: [u8; 32],
    pub bump: u8,
}

impl InvestorRecord {
    pub const LEN: usize = 32 + 8 + 1 + 32 + 1;
}

#[event]
pub mod events {
    use super::*;

    #[event]
    pub struct PoolInitialized {
        pub pool_id: String,
        pub authority: Pubkey,
        pub principal_target: u64,
        pub senior_ratio_bps: u64,
    }

    #[event]
    pub struct InvestorRegistered {
        pub investor: Pubkey,
        pub accredited_until: i64,
    }

    #[event]
    pub struct FundsInvested {
        pub pool_id: String,
        pub investor: Pubkey,
        pub amount: u64,
        pub is_senior: bool,
    }

    #[event]
    pub struct PoolFullyFunded {
        pub pool_id: String,
        pub start_timestamp: i64,
        pub maturity_timestamp: i64,
    }

    #[event]
    pub struct RepaymentProcessed {
        pub pool_id: String,
        pub total_repayment: u64,
        pub senior_portion: u64,
        pub junior_portion: u64,
        pub timestamp: i64,
    }
}

pub use events::*;

#[error_code]
pub enum CreditError {
    #[msg("Pool identifier string exceeds maximum length of 32 bytes.")]
    IdentifierTooLong,
    #[msg("Senior ratio basis points cannot exceed 10,000.")]
    InvalidRatio,
    #[msg("Requested operation amount must be strictly greater than zero.")]
    ZeroAmount,
    #[msg("Credit tenor must be greater than zero seconds.")]
    InvalidTenor,
    #[msg("Supplied KYC accreditation timestamp is in the past.")]
    ExpiredKyc,
    #[msg("The credit pool has already satisfied its funding target.")]
    PoolAlreadyFunded,
    #[msg("Investor account has been placed on the sanctions or blacklist.")]
    BlacklistedAccount,
    #[msg("Investor account record does not match transaction signer.")]
    UnauthorizedInvestor,
    #[msg("Tranche allocation limit would be exceeded by this deposit.")]
    TrancheCapacityExceeded,
    #[msg("Credit pool is not fully funded yet.")]
    PoolNotFunded,
    #[msg("Arithmetic overflow encountered during calculation.")]
    MathOverflow,
    #[msg("Arithmetic underflow encountered during calculation.")]
    MathUnderflow,
}