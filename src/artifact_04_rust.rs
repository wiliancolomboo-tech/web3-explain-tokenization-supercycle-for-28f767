use anchor_lang::prelude::*;
use anchor_spl::token_2022::{self, MintTo, Token2022, TransferChecked};
use anchor_spl::token_interface::{Mint, TokenAccount};

declare_id!("RWA1111111111111111111111111111111111111111");

pub const SENIOR_TRANCHE_YIELD_BPS: u64 = 800; // 8.00% Target APY
pub const JUNIOR_TRANCHE_YIELD_BPS: u64 = 1800; // 18.00% Subordinated Target APY
pub const BASIS_POINTS_DIVISOR: u64 = 10000;
pub const SECONDS_PER_YEAR: u64 = 31_536_000;

#[program]
pub mod rwa_credit_supercycle {
    use super::*;

    /// Initialize a Global Private Credit Loan Pool
    pub fn initialize_pool(
        ctx: Context<InitializePool>,
        pool_name: String,
        loan_principal_capacity: u64,
        duration_seconds: i64,
        senior_ratio_bps: u64,
    ) -> Result<()> {
        require!(
            senior_ratio_bps <= BASIS_POINTS_DIVISOR,
            RwaError::InvalidRatio
        );
        require!(
            loan_principal_capacity > 0,
            RwaError::InvalidPrincipalAmount
        );

        let pool = &mut ctx.accounts.pool;
        pool.admin = ctx.accounts.admin.key();
        pool.borrower = ctx.accounts.borrower.key();
        pool.usdc_mint = ctx.accounts.usdc_mint.key();
        pool.senior_mint = ctx.accounts.senior_mint.key();
        pool.junior_mint = ctx.accounts.junior_mint.key();
        pool.usdc_vault = ctx.accounts.usdc_vault.key();
        pool.pool_name = pool_name;
        pool.principal_capacity = loan_principal_capacity;
        pool.senior_capacity = (loan_principal_capacity * senior_ratio_bps) / BASIS_POINTS_DIVISOR;
        pool.junior_capacity = loan_principal_capacity.saturating_sub(pool.senior_capacity);
        pool.senior_deposited = 0;
        pool.junior_deposited = 0;
        pool.senior_debt_serviced = 0;
        pool.junior_debt_serviced = 0;
        pool.duration_seconds = duration_seconds;
        pool.loan_start_timestamp = 0;
        pool.loan_maturity_timestamp = 0;
        pool.pool_state = PoolState::Funding;
        pool.bump = ctx.bumps.pool;

        emit!(PoolInitializedEvent {
            pool: pool.key(),
            admin: pool.admin,
            borrower: pool.borrower,
            capacity: loan_principal_capacity,
            senior_capacity: pool.senior_capacity,
            junior_capacity: pool.junior_capacity,
        });

        Ok(())
    }

    /// Whitelist an institutional identity for KYC/AML Compliance
    pub fn set_kyc_status(
        ctx: Context<SetKycStatus>,
        investor: Pubkey,
        is_kyced: bool,
        jurisdiction: [u8; 2],
    ) -> Result<()> {
        let registry = &mut ctx.accounts.kyc_registry;
        registry.investor = investor;
        registry.is_kyced = is_kyced;
        registry.jurisdiction = jurisdiction;
        registry.updated_at = Clock::get()?.unix_timestamp;
        registry.bump = ctx.bumps.kyc_registry;

        emit!(KycUpdatedEvent {
            investor,
            is_kyced,
            jurisdiction,
        });

        Ok(())
    }

    /// Deposit USDC into Senior or Junior Tranche Vault
    pub fn deposit_tranche(
        ctx: Context<DepositTranche>,
        amount: u64,
        is_senior: bool,
    ) -> Result<()> {
        let pool = &mut ctx.accounts.pool;
        let kyc = &ctx.accounts.kyc_registry;

        require!(pool.pool_state == PoolState::Funding, RwaError::PoolNotFunding);
        require!(kyc.is_kyced, RwaError::KycRequired);
        require!(amount > 0, RwaError::ZeroDeposit);

        if is_senior {
            require!(
                pool.senior_deposited.checked_add(amount).unwrap() <= pool.senior_capacity,
                RwaError::CapacityExceeded
            );
            pool.senior_deposited = pool.senior_deposited.checked_add(amount).unwrap();
        } else {
            require!(
                pool.junior_deposited.checked_add(amount).unwrap() <= pool.junior_capacity,
                RwaError::CapacityExceeded
            );
            pool.junior_deposited = pool.junior_deposited.checked_add(amount).unwrap();
        }

        // Transfer USDC from Investor to Pool Vault
        let transfer_cpi_accounts = TransferChecked {
            from: ctx.accounts.investor_usdc.to_account_info(),
            mint: ctx.accounts.usdc_mint.to_account_info(),
            to: ctx.accounts.usdc_vault.to_account_info(),
            authority: ctx.accounts.investor.to_account_info(),
        };
        let cpi_ctx = CpiContext::new(
            ctx.accounts.token_2022_program.to_account_info(),
            transfer_cpi_accounts,
        );
        token_2022::transfer_checked(cpi_ctx, amount, 6)?;

        // Mint Tranche Tokens (Senior or Junior) to Investor
        let pool_key = pool.key();
        let seeds = &[
            b"credit_pool",
            pool_key.as_ref(),
            &[pool.bump],
        ];
        let signer = &[&seeds[..]];

        let mint_target = if is_senior {
            ctx.accounts.senior_mint.to_account_info()
        } else {
            ctx.accounts.junior_mint.to_account_info()
        };

        let mint_cpi_accounts = MintTo {
            mint: mint_target,
            to: ctx.accounts.investor_tranche_token.to_account_info(),
            authority: pool.to_account_info(),
        };
        let mint_ctx = CpiContext::new_with_signer(
            ctx.accounts.token_2022_program.to_account_info(),
            mint_cpi_accounts,
            signer,
        );
        token_2022::mint_to(mint_ctx, amount)?;

        emit!(DepositEvent {
            pool: pool.key(),
            investor: ctx.accounts.investor.key(),
            amount,
            is_senior,
        });

        Ok(())
    }

    /// Borrower draws down the total pooled capital once fully funded
    pub fn drawdown_principal(ctx: Context<DrawdownPrincipal>) -> Result<()> {
        let pool = &mut ctx.accounts.pool;
        require!(
            pool.pool_state == PoolState::Funding,
            RwaError::InvalidPoolState
        );
        require!(
            pool.senior_deposited == pool.senior_capacity && pool.junior_deposited == pool.junior_capacity,
            RwaError::PoolNotFullyFunded
        );

        let total_principal = pool.principal_capacity;
        let clock = Clock::get()?;

        pool.pool_state = PoolState::Active;
        pool.loan_start_timestamp = clock.unix_timestamp;
        pool.loan_maturity_timestamp = clock.unix_timestamp + pool.duration_seconds;

        let pool_key = pool.key();
        let seeds = &[
            b"credit_pool",
            pool_key.as_ref(),
            &[pool.bump],
        ];
        let signer = &[&seeds[..]];

        let transfer_cpi_accounts = TransferChecked {
            from: ctx.accounts.usdc_vault.to_account_info(),
            mint: ctx.accounts.usdc_mint.to_account_info(),
            to: ctx.accounts.borrower_usdc.to_account_info(),
            authority: pool.to_account_info(),
        };
        let cpi_ctx = CpiContext::new_with_signer(
            ctx.accounts.token_2022_program.to_account_info(),
            transfer_cpi_accounts,
            signer,
        );
        token_2022::transfer_checked(cpi_ctx, total_principal, 6)?;

        emit!(DrawdownEvent {
            pool: pool.key(),
            borrower: pool.borrower,
            amount: total_principal,
            start_timestamp: pool.loan_start_timestamp,
            maturity_timestamp: pool.loan_maturity_timestamp,
        });

        Ok(())
    }

    /// Borrower repays interest and principal following deterministic waterfall
    pub fn repay_waterfall(
        ctx: Context<RepayWaterfall>,
        senior_interest: u64,
        junior_interest: u64,
        principal_repaid: u64,
    ) -> Result<()> {
        let pool = &mut ctx.accounts.pool;
        require!(pool.pool_state == PoolState::Active, RwaError::PoolNotActive);

        let total_repayment = senior_interest
            .checked_add(junior_interest)
            .unwrap()
            .checked_add(principal_repaid)
            .unwrap();

        let transfer_cpi_accounts = TransferChecked {
            from: ctx.accounts.borrower_usdc.to_account_info(),
            mint: ctx.accounts.usdc_mint.to_account_info(),
            to: ctx.accounts.usdc_vault.to_account_info(),
            authority: ctx.accounts.borrower.to_account_info(),
        };
        let cpi_ctx = CpiContext::new(
            ctx.accounts.token_2022_program.to_account_info(),
            transfer_cpi_accounts,
        );
        token_2022::transfer_checked(cpi_ctx, total_repayment, 6)?;

        pool.senior_debt_serviced = pool.senior_debt_serviced.checked_add(senior_interest).unwrap();
        pool.junior_debt_serviced = pool.junior_debt_serviced.checked_add(junior_interest).unwrap();

        if pool.senior_debt_serviced >= pool.senior_capacity
            && pool.junior_debt_serviced >= pool.junior_capacity
        {
            pool.pool_state = PoolState::Matured;
        }

        emit!(WaterfallRepaymentEvent {
            pool: pool.key(),
            total_repayment,
            senior_interest,
            junior_interest,
            principal_repaid,
        });

        Ok(())
    }

    /// Token-2022 Transfer Hook Entrypoint: Enforces KYC on every transfer instruction
    pub fn transfer_hook(ctx: Context<TransferHookContext>, amount: u64) -> Result<()> {
        let sender_kyc = &ctx.accounts.sender_kyc_registry;
        let receiver_kyc = &ctx.accounts.receiver_kyc_registry;

        require!(sender_kyc.is_kyced, RwaError::SenderNotKYCed);
        require!(receiver_kyc.is_kyced, RwaError::ReceiverNotKYCed);
        require!(amount > 0, RwaError::InvalidTransferAmount);

        msg!("Token-2022 Transfer Hook: Compliance Verified for Transfer of {} units", amount);
        Ok(())
    }
}

// -----------------------------------------------------------------------------
// ACCOUNT VALIDATION CONTEXTS
// -----------------------------------------------------------------------------

#[derive(Accounts)]
#[instruction(pool_name: String)]
pub struct InitializePool<'info> {
    #[account(
        init,
        payer = admin,
        space = 8 + CreditPool::INIT_SPACE,
        seeds = [b"credit_pool", admin.key().as_ref(), pool_name.as_bytes()],
        bump
    )]
    pub pool: Account<'info, CreditPool>,

    #[account(mut)]
    pub admin: Signer<'info>,

    /// CHECK: Safe institutional counterparty
    pub borrower: AccountInfo<'info>,

    pub usdc_mint: InterfaceAccount<'info, Mint>,

    #[account(
        init,
        payer = admin,
        seeds = [b"senior_mint", pool.key().as_ref()],
        bump,
        mint::decimals = 6,
        mint::authority = pool,
        mint::token_program = token_2022_program,
    )]
    pub senior_mint: InterfaceAccount<'info, Mint>,

    #[account(
        init,
        payer = admin,
        seeds = [b"junior_mint", pool.key().as_ref()],
        bump,
        mint::decimals = 6,
        mint::authority = pool,
        mint::token_program = token_2022_program,
    )]
    pub junior_mint: InterfaceAccount<'info, Mint>,

    #[account(
        init,
        payer = admin,
        seeds = [b"usdc_vault", pool.key().as_ref()],
        bump,
        token::mint = usdc_mint,
        token::authority = pool,
        token::token_program = token_2022_program,
    )]
    pub usdc_vault: InterfaceAccount<'info, TokenAccount>,

    pub system_program: Program<'info, System>,
    pub token_2022_program: Program<'info, Token2022>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
pub struct SetKycStatus<'info> {
    #[account(
        init_if_needed,
        payer = authority,
        space = 8 + KycRegistry::INIT_SPACE,
        seeds = [b"kyc_registry", investor.key().as_ref()],
        bump
    )]
    pub kyc_registry: Account<'info, KycRegistry>,

    /// CHECK: Protocol administrator / KYC verification gate
    pub investor: AccountInfo<'info>,

    #[account(mut)]
    pub authority: Signer<'info>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct DepositTranche<'info> {
    #[account(mut)]
    pub pool: Account<'info, CreditPool>,

    #[account(
        seeds = [b"kyc_registry", investor.key().as_ref()],
        bump = kyc_registry.bump
    )]
    pub kyc_registry: Account<'info, KycRegistry>,

    #[account(mut)]
    pub investor: Signer<'info>,

    #[account(mut)]
    pub investor_usdc: InterfaceAccount<'info, TokenAccount>,

    #[account(
        mut,
        seeds = [b"usdc_vault", pool.key().as_ref()],
        bump
    )]
    pub usdc_vault: InterfaceAccount<'info, TokenAccount>,

    pub usdc_mint: InterfaceAccount<'info, Mint>,

    #[account(mut)]
    pub senior_mint: InterfaceAccount<'info, Mint>,

    #[account(mut)]
    pub junior_mint: InterfaceAccount<'info, Mint>,

    #[account(mut)]
    pub investor_tranche_token: InterfaceAccount<'info, TokenAccount>,

    pub token_2022_program: Program<'info, Token2022>,
}

#[derive(Accounts)]
pub struct DrawdownPrincipal<'info> {
    #[account(
        mut,
        has_one = borrower,
        seeds = [b"credit_pool", pool.admin.as_ref(), pool.pool_name.as_bytes()],
        bump = pool.bump
    )]
    pub pool: Account<'info, CreditPool>,

    #[account(mut)]
    pub borrower: Signer<'info>,

    #[account(
        mut,
        seeds = [b"usdc_vault", pool.key().as_ref()],
        bump
    )]
    pub usdc_vault: InterfaceAccount<'info, TokenAccount>,

    pub usdc_mint: InterfaceAccount<'info, Mint>,

    #[account(mut)]
    pub borrower_usdc: InterfaceAccount<'info, TokenAccount>,

    pub token_2022_program: Program<'info, Token2022>,
}

#[derive(Accounts)]
pub struct RepayWaterfall<'info> {
    #[account(
        mut,
        has_one = borrower,
        seeds = [b"credit_pool", pool.admin.as_ref(), pool.pool_name.as_bytes()],
        bump = pool.bump
    )]
    pub pool: Account<'info, CreditPool>,

    #[account(mut)]
    pub borrower: Signer<'info>,

    #[account(mut)]
    pub borrower_usdc: InterfaceAccount<'info, TokenAccount>,

    #[account(
        mut,
        seeds = [b"usdc_vault", pool.key().as_ref()],
        bump
    )]
    pub usdc_vault: InterfaceAccount<'info, TokenAccount>,

    pub usdc_mint: InterfaceAccount<'info, Mint>,
    pub token_2022_program: Program<'info, Token2022>,
}

#[derive(Accounts)]
pub struct TransferHookContext<'info> {
    /// CHECK: Checked by transfer hook interface
    pub source_token: AccountInfo<'info>,
    /// CHECK: Checked by transfer hook interface
    pub mint: AccountInfo<'info>,
    /// CHECK: Checked by transfer hook interface
    pub destination_token: AccountInfo<'info>,
    /// CHECK: Checked by transfer hook interface
    pub owner: AccountInfo<'info>,
    /// CHECK: Checked by transfer hook interface
    pub extra_account_metas: AccountInfo<'info>,

    #[account(
        seeds = [b"kyc_registry", sender_authority.key().as_ref()],
        bump = sender_kyc_registry.bump
    )]
    pub sender_kyc_registry: Account<'info, KycRegistry>,

    /// CHECK: Signer or owner of source
    pub sender_authority: AccountInfo<'info>,

    #[account(
        seeds = [b"kyc_registry", receiver_authority.key().as_ref()],
        bump = receiver_kyc_registry.bump
    )]
    pub receiver_kyc_registry: Account<'info, KycRegistry>,

    /// CHECK: Owner of destination
    pub receiver_authority: AccountInfo<'info>,
}

// -----------------------------------------------------------------------------
// STATE STRUCTS
// -----------------------------------------------------------------------------

#[account]
#[derive(InitSpace)]
pub struct CreditPool {
    pub admin: Pubkey,
    pub borrower: Pubkey,
    pub usdc_mint: Pubkey,
    pub senior_mint: Pubkey,
    pub junior_mint: Pubkey,
    pub usdc_vault: Pubkey,
    #[max_len(32)]
    pub pool_name: String,
    pub principal_capacity: u64,
    pub senior_capacity: u64,
    pub junior_capacity: u64,
    pub senior_deposited: u64,
    pub junior_deposited: u64,
    pub senior_debt_serviced: u64,
    pub junior_debt_serviced: u64,
    pub duration_seconds: i64,
    pub loan_start_timestamp: i64,
    pub loan_maturity_timestamp: i64,
    pub pool_state: PoolState,
    pub bump: u8,
}

#[account]
#[derive(InitSpace)]
pub struct KycRegistry {
    pub investor: Pubkey,
    pub is_kyced: bool,
    pub jurisdiction: [u8; 2],
    pub updated_at: i64,
    pub bump: u8,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq, InitSpace)]
pub enum PoolState {
    Funding,
    Active,
    Matured,
    Defaulted,
}

// -----------------------------------------------------------------------------
// EVENTS & ERRORS
// -----------------------------------------------------------------------------

#[event]
pub struct PoolInitializedEvent {
    pub pool: Pubkey,
    pub admin: Pubkey,
    pub borrower: Pubkey,
    pub capacity: u64,
    pub senior_capacity: u64,
    pub junior_capacity: u64,
}

#[event]
pub struct KycUpdatedEvent {
    pub investor: Pubkey,
    pub is_kyced: bool,
    pub jurisdiction: [u8; 2],
}

#[event]
pub struct DepositEvent {
    pub pool: Pubkey,
    pub investor: Pubkey,
    pub amount: u64,
    pub is_senior: bool,
}

#[event]
pub struct DrawdownEvent {
    pub pool: Pubkey,
    pub borrower: Pubkey,
    pub amount: u64,
    pub start_timestamp: i64,
    pub maturity_timestamp: i64,
}

#[event]
pub struct WaterfallRepaymentEvent {
    pub pool: Pubkey,
    pub total_repayment: u64,
    pub senior_interest: u64,
    pub junior_interest: u64,
    pub principal_repaid: u64,
}

#[error_code]
pub enum RwaError {
    #[msg("Invalid ratio: must be <= 10000 basis points")]
    InvalidRatio,
    #[msg("Invalid principal amount")]
    InvalidPrincipalAmount,
    #[msg("Pool is not in Funding state")]
    PoolNotFunding,
    #[msg("Pool is not in Active state")]
    PoolNotActive,
    #[msg("Investor fails KYC verification check")]
    KycRequired,
    #[msg("Deposit amount must be greater than zero")]
    ZeroDeposit,
    #[msg("Tranche capacity exceeded")]
    CapacityExceeded,
    #[msg("Pool is not yet fully funded")]
    PoolNotFullyFunded,
    #[msg("Invalid pool state transition")]
    InvalidPoolState,
    #[msg("Transfer hook rejected: Sender is not KYC validated")]
    SenderNotKYCed,
    #[msg("Transfer hook rejected: Receiver is not KYC validated")]
    ReceiverNotKYCed,
    #[msg("Transfer amount must be greater than zero")]
    InvalidTransferAmount,
}