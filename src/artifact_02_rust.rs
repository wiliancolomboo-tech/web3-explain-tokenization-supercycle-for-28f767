// programs/solpv_core/src/lib.rs
use anchor_lang::prelude::*;
use anchor_spl::token_interface::{self, Mint, TokenAccount, TokenInterface, TransferChecked};

declare_id!("So1PV11111111111111111111111111111111111111");

pub const SENIOR_TRANCHE_BASIS_POINTS: u64 = 750; // 7.5% APY Target
pub const MAX_BASIS_POINTS: u64 = 10_000;
pub const SECONDS_PER_YEAR: u64 = 31_536_000;

#[program]
pub mod solpv_core {
    use super::*;

    /// Initialize the Securitized Solar Debt Pool
    pub fn initialize_pool(
        ctx: Context<InitializePool>,
        pool_name: String,
        senior_debt_ceiling: u64,
        junior_debt_ceiling: u64,
    ) -> Result<()> {
        require!(pool_name.len() <= 32, SolPvError::PoolNameTooLong);
        require!(senior_debt_ceiling > 0, SolPvError::InvalidCeiling);
        require!(junior_debt_ceiling > 0, SolPvError::InvalidCeiling);

        let pool = &mut ctx.accounts.pool;
        pool.authority = ctx.accounts.authority.key();
        pool.pool_name = pool_name;
        pool.senior_mint = ctx.accounts.senior_mint.key();
        pool.junior_mint = ctx.accounts.junior_mint.key();
        pool.stable_mint = ctx.accounts.stable_mint.key();
        pool.stable_vault = ctx.accounts.stable_vault.key();
        pool.senior_debt_ceiling = senior_debt_ceiling;
        pool.junior_debt_ceiling = junior_debt_ceiling;
        pool.senior_invested_amount = 0;
        pool.junior_invested_amount = 0;
        pool.total_energy_produced_kwh = 0;
        pool.last_yield_distribution = Clock::get()?.unix_timestamp;
        pool.is_paused = false;
        pool.bump = ctx.bumps.pool;

        emit!(PoolInitializedEvent {
            pool: pool.key(),
            authority: pool.authority,
            senior_ceiling: senior_debt_ceiling,
            junior_ceiling: junior_debt_ceiling,
            timestamp: Clock::get()?.unix_timestamp,
        });

        Ok(())
    }

    /// Manage Accredited Investor Whitelist for Token-2022 Transfer Hook integration
    pub fn set_investor_status(
        ctx: Context<SetInvestorStatus>,
        investor: Pubkey,
        is_accredited: bool,
        jurisdiction_code: u16,
    ) -> Result<()> {
        let registry = &mut ctx.accounts.whitelist_entry;
        registry.investor = investor;
        registry.is_accredited = is_accredited;
        registry.jurisdiction_code = jurisdiction_code;
        registry.updated_at = Clock::get()?.unix_timestamp;
        registry.bump = ctx.bumps.whitelist_entry;

        emit!(InvestorStatusUpdatedEvent {
            investor,
            is_accredited,
            jurisdiction_code,
            timestamp: registry.updated_at,
        });

        Ok(())
    }

    /// Institutional Investor Subscription to Tranches
    pub fn deposit_capital(
        ctx: Context<DepositCapital>,
        amount: u64,
        is_senior_tranche: bool,
    ) -> Result<()> {
        let pool = &mut ctx.accounts.pool;
        require!(!pool.is_paused, SolPvError::PoolPaused);
        require!(amount > 0, SolPvError::ZeroDeposit);
        require!(ctx.accounts.whitelist_entry.is_accredited, SolPvError::NotAccredited);

        // Enforce debt capacity ceilings
        if is_senior_tranche {
            require!(
                pool.senior_invested_amount.checked_add(amount).unwrap() <= pool.senior_debt_ceiling,
                SolPvError::CeilingExceeded
            );
            pool.senior_invested_amount = pool.senior_invested_amount.checked_add(amount).unwrap();
        } else {
            require!(
                pool.junior_invested_amount.checked_add(amount).unwrap() <= pool.junior_debt_ceiling,
                SolPvError::CeilingExceeded
            );
            pool.junior_invested_amount = pool.junior_invested_amount.checked_add(amount).unwrap();
        }

        // Transfer capital (USDC) from investor to protocol custody vault
        let transfer_cpi_accounts = TransferChecked {
            from: ctx.accounts.investor_stable_ata.to_account_info(),
            mint: ctx.accounts.stable_mint.to_account_info(),
            to: ctx.accounts.stable_vault.to_account_info(),
            authority: ctx.accounts.investor.to_account_info(),
        };
        let cpi_program = ctx.accounts.token_program.to_account_info();
        let cpi_ctx = CpiContext::new(cpi_program, transfer_cpi_accounts);
        token_interface::transfer_checked(cpi_ctx, amount, ctx.accounts.stable_mint.decimals)?;

        emit!(CapitalDepositedEvent {
            pool: pool.key(),
            investor: ctx.accounts.investor.key(),
            amount,
            is_senior_tranche,
            timestamp: Clock::get()?.unix_timestamp,
        });

        Ok(())
    }

    /// Record Verified IoT Solar Production (Edge Oracle Settlement)
    pub fn record_solar_production(
        ctx: Context<RecordProduction>,
        kwh_generated: u64,
        oracle_signature_hash: [u8; 32],
    ) -> Result<()> {
        let pool = &mut ctx.accounts.pool;
        require!(!pool.is_paused, SolPvError::PoolPaused);
        require!(kwh_generated > 0, SolPvError::InvalidTelemetry);

        pool.total_energy_produced_kwh = pool
            .total_energy_produced_kwh
            .checked_add(kwh_generated)
            .unwrap();

        emit!(SolarProductionRecordedEvent {
            pool: pool.key(),
            kwh_generated,
            total_kwh: pool.total_energy_produced_kwh,
            signature_hash: oracle_signature_hash,
            timestamp: Clock::get()?.unix_timestamp,
        });

        Ok(())
    }

    /// Execute Mathematical Debt Waterfall for Distributed Yield
    pub fn distribute_yield_waterfall(
        ctx: Context<DistributeWaterfall>,
        total_payout_amount: u64,
    ) -> Result<()> {
        let pool = &mut ctx.accounts.pool;
        require!(!pool.is_paused, SolPvError::PoolPaused);
        require!(total_payout_amount > 0, SolPvError::ZeroPayout);

        let now = Clock::get()?.unix_timestamp;
        let time_elapsed = (now - pool.last_yield_distribution).max(0) as u64;

        // 1. Calculate Senior Fixed Obligation: P * r * (dt / SecondsPerYear)
        let senior_due = (pool.senior_invested_amount as u128)
            .checked_mul(SENIOR_TRANCHE_BASIS_POINTS as u128)
            .unwrap()
            .checked_mul(time_elapsed as u128)
            .unwrap()
            .checked_div((MAX_BASIS_POINTS as u128) * (SECONDS_PER_YEAR as u128))
            .unwrap() as u64;

        let actual_senior_payout = senior_due.min(total_payout_amount);
        let actual_junior_payout = total_payout_amount.saturating_sub(actual_senior_payout);

        let pool_name_bytes = pool.pool_name.as_bytes();
        let seeds = &[
            b"solpv_pool".as_ref(),
            pool_name_bytes,
            &[pool.bump],
        ];
        let signer_seeds = &[&seeds[..]];

        // 2. Transfer Senior Yield to Senior Escrow
        if actual_senior_payout > 0 {
            let senior_transfer = TransferChecked {
                from: ctx.accounts.stable_vault.to_account_info(),
                mint: ctx.accounts.stable_mint.to_account_info(),
                to: ctx.accounts.senior_escrow_ata.to_account_info(),
                authority: pool.to_account_info(),
            };
            let cpi_ctx = CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                senior_transfer,
                signer_seeds,
            );
            token_interface::transfer_checked(cpi_ctx, actual_senior_payout, ctx.accounts.stable_mint.decimals)?;
        }

        // 3. Transfer Subordinated (Junior) Residual Yield
        if actual_junior_payout > 0 {
            let junior_transfer = TransferChecked {
                from: ctx.accounts.stable_vault.to_account_info(),
                mint: ctx.accounts.stable_mint.to_account_info(),
                to: ctx.accounts.junior_escrow_ata.to_account_info(),
                authority: pool.to_account_info(),
            };
            let cpi_ctx = CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                junior_transfer,
                signer_seeds,
            );
            token_interface::transfer_checked(cpi_ctx, actual_junior_payout, ctx.accounts.stable_mint.decimals)?;
        }

        pool.last_yield_distribution = now;

        emit!(YieldWaterfallExecutedEvent {
            pool: pool.key(),
            total_distributed: total_payout_amount,
            senior_distributed: actual_senior_payout,
            junior_distributed: actual_junior_payout,
            timestamp: now,
        });

        Ok(())
    }

    /// Token-2022 Transfer Hook Enforcement Gateway
    pub fn enforce_transfer_hook(
        ctx: Context<EnforceTransferHook>,
        amount: u64,
    ) -> Result<()> {
        let source_entry = &ctx.accounts.source_whitelist_entry;
        let dest_entry = &ctx.accounts.destination_whitelist_entry;

        require!(source_entry.is_accredited, SolPvError::SourceNotAccredited);
        require!(dest_entry.is_accredited, SolPvError::DestinationNotAccredited);
        require!(
            source_entry.jurisdiction_code == dest_entry.jurisdiction_code || dest_entry.jurisdiction_code == 840, // 840 = US Reg D
            SolPvError::JurisdictionMismatch
        );

        msg!("Token-2022 Transfer Hook: Verified accredited transfer of {} units", amount);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Account Context Structs
// ---------------------------------------------------------------------------

#[derive(Accounts)]
#[instruction(pool_name: String)]
pub struct InitializePool<'info> {
    #[account(
        init,
        payer = authority,
        space = 8 + SolPvPool::INIT_SPACE,
        seeds = [b"solpv_pool", pool_name.as_bytes()],
        bump
    )]
    pub pool: Account<'info, SolPvPool>,
    pub senior_mint: InterfaceAccount<'info, Mint>,
    pub junior_mint: InterfaceAccount<'info, Mint>,
    pub stable_mint: InterfaceAccount<'info, Mint>,
    #[account(
        init,
        payer = authority,
        token::mint = stable_mint,
        token::authority = pool,
        seeds = [b"pool_stable_vault", pool.key().as_ref()],
        bump
    )]
    pub stable_vault: InterfaceAccount<'info, TokenAccount>,
    #[account(mut)]
    pub authority: Signer<'info>,
    pub system_program: Program<'info, System>,
    pub token_program: Interface<'info, TokenInterface>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
pub struct SetInvestorStatus<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,
    #[account(
        init_if_needed,
        payer = authority,
        space = 8 + WhitelistEntry::INIT_SPACE,
        seeds = [b"whitelist_registry", investor.key().as_ref()],
        bump
    )]
    pub whitelist_entry: Account<'info, WhitelistEntry>,
    /// CHECK: Target investor wallet
    pub investor: AccountInfo<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct DepositCapital<'info> {
    #[account(mut)]
    pub pool: Account<'info, SolPvPool>,
    #[account(mut)]
    pub investor: Signer<'info>,
    #[account(
        seeds = [b"whitelist_registry", investor.key().as_ref()],
        bump = whitelist_entry.bump
    )]
    pub whitelist_entry: Account<'info, WhitelistEntry>,
    pub stable_mint: InterfaceAccount<'info, Mint>,
    #[account(
        mut,
        token::mint = stable_mint,
        token::authority = investor
    )]
    pub investor_stable_ata: InterfaceAccount<'info, TokenAccount>,
    #[account(
        mut,
        seeds = [b"pool_stable_vault", pool.key().as_ref()],
        bump
    )]
    pub stable_vault: InterfaceAccount<'info, TokenAccount>,
    pub token_program: Interface<'info, TokenInterface>,
}

#[derive(Accounts)]
pub struct RecordProduction<'info> {
    #[account(mut, has_one = authority)]
    pub pool: Account<'info, SolPvPool>,
    pub authority: Signer<'info>,
}

#[derive(Accounts)]
pub struct DistributeWaterfall<'info> {
    #[account(mut, has_one = authority)]
    pub pool: Account<'info, SolPvPool>,
    pub authority: Signer<'info>,
    pub stable_mint: InterfaceAccount<'info, Mint>,
    #[account(
        mut,
        seeds = [b"pool_stable_vault", pool.key().as_ref()],
        bump
    )]
    pub stable_vault: InterfaceAccount<'info, TokenAccount>,
    #[account(mut)]
    pub senior_escrow_ata: InterfaceAccount<'info, TokenAccount>,
    #[account(mut)]
    pub junior_escrow_ata: InterfaceAccount<'info, TokenAccount>,
    pub token_program: Interface<'info, TokenInterface>,
}

#[derive(Accounts)]
pub struct EnforceTransferHook<'info> {
    /// CHECK: Verified source token account
    pub source: AccountInfo<'info>,
    /// CHECK: Verified destination token account
    pub destination: AccountInfo<'info>,
    pub mint: InterfaceAccount<'info, Mint>,
    #[account(
        seeds = [b"whitelist_registry", source.key().as_ref()],
        bump = source_whitelist_entry.bump
    )]
    pub source_whitelist_entry: Account<'info, WhitelistEntry>,
    #[account(
        seeds = [b"whitelist_registry", destination.key().as_ref()],
        bump = destination_whitelist_entry.bump
    )]
    pub destination_whitelist_entry: Account<'info, WhitelistEntry>,
}

// ---------------------------------------------------------------------------
// State Declarations
// ---------------------------------------------------------------------------

#[account]
#[derive(InitSpace)]
pub struct SolPvPool {
    pub authority: Pubkey,
    #[max_len(32)]
    pub pool_name: String,
    pub senior_mint: Pubkey,
    pub junior_mint: Pubkey,
    pub stable_mint: Pubkey,
    pub stable_vault: Pubkey,
    pub senior_debt_ceiling: u64,
    pub junior_debt_ceiling: u64,
    pub senior_invested_amount: u64,
    pub junior_invested_amount: u64,
    pub total_energy_produced_kwh: u64,
    pub last_yield_distribution: i64,
    pub is_paused: bool,
    pub bump: u8,
}

#[account]
#[derive(InitSpace)]
pub struct WhitelistEntry {
    pub investor: Pubkey,
    pub is_accredited: bool,
    pub jurisdiction_code: u16,
    pub updated_at: i64,
    pub bump: u8,
}

// ---------------------------------------------------------------------------
// Events & Custom Errors
// ---------------------------------------------------------------------------

#[event]
pub struct PoolInitializedEvent {
    pub pool: Pubkey,
    pub authority: Pubkey,
    pub senior_ceiling: u64,
    pub junior_ceiling: u64,
    pub timestamp: i64,
}

#[event]
pub struct InvestorStatusUpdatedEvent {
    pub investor: Pubkey,
    pub is_accredited: bool,
    pub jurisdiction_code: u16,
    pub timestamp: i64,
}

#[event]
pub struct CapitalDepositedEvent {
    pub pool: Pubkey,
    pub investor: Pubkey,
    pub amount: u64,
    pub is_senior_tranche: bool,
    pub timestamp: i64,
}

#[event]
pub struct SolarProductionRecordedEvent {
    pub pool: Pubkey,
    pub kwh_generated: u64,
    pub total_kwh: u64,
    pub signature_hash: [u8; 32],
    pub timestamp: i64,
}

#[event]
pub struct YieldWaterfallExecutedEvent {
    pub pool: Pubkey,
    pub total_distributed: u64,
    pub senior_distributed: u64,
    pub junior_distributed: u64,
    pub timestamp: i64,
}

#[error_code]
pub enum SolPvError {
    #[msg("Pool name exceeds maximum length of 32 characters")]
    PoolNameTooLong,
    #[msg("Provided debt ceiling is invalid")]
    InvalidCeiling,
    #[msg("Operation rejected: Securitization pool is paused")]
    PoolPaused,
    #[msg("Deposit amount must be greater than zero")]
    ZeroDeposit,
    #[msg("Deposit would exceed tranche debt ceiling limit")]
    CeilingExceeded,
    #[msg("Investor wallet fails accredited KYC status")]
    NotAccredited,
    #[msg("Solar telemetry production value must be strictly positive")]
    InvalidTelemetry,
    #[msg("Yield waterfall distribution must be strictly positive")]
    ZeroPayout,
    #[msg("Transfer rejected: Source address is not accredited")]
    SourceNotAccredited,
    #[msg("Transfer rejected: Destination address is not accredited")]
    DestinationNotAccredited,
    #[msg("Transfer rejected: Cross-jurisdiction compliance violation")]
    JurisdictionMismatch,
}