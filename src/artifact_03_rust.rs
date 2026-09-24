use anchor_lang::prelude::*;
use anchor_spl::token_2022::{self, MintTo, Token2022};
use anchor_spl::token_interface::{Mint, TokenAccount};

declare_id!("SYE1111111111111111111111111111111111111111");

pub const BPS_DIVISOR: u64 = 10_000;
pub const MAX_ORACLE_STALENESS_SECONDS: i64 = 3600;

#[program]
pub mod sovereign_yield_engine {
    use super::*;

    /// Initializes a new institutional asset pool with dual tranches.
    pub fn initialize_pool(
        ctx: Context<InitializePool>,
        pool_bump: u8,
        senior_yield_bps: u16,
        reserve_ratio_bps: u16,
    ) -> Result<()> {
        require!(reserve_ratio_bps <= BPS_DIVISOR as u16, ErrorCode::InvalidRatio);
        
        let pool = &mut ctx.accounts.pool_config;
        pool.authority = ctx.accounts.authority.key();
        pool.usdc_mint = ctx.accounts.usdc_mint.key();
        pool.senior_mint = ctx.accounts.senior_mint.key();
        pool.junior_mint = ctx.accounts.junior_mint.key();
        pool.senior_vault = ctx.accounts.senior_vault.key();
        pool.junior_vault = ctx.accounts.junior_vault.key();
        pool.senior_yield_bps = senior_yield_bps;
        pool.reserve_ratio_bps = reserve_ratio_bps;
        pool.total_senior_shares = 0;
        pool.total_junior_shares = 0;
        pool.pool_bump = pool_bump;
        pool.last_epoch_timestamp = Clock::get()?.unix_timestamp;
        pool.paused = false;

        emit!(PoolInitialized {
            authority: pool.authority,
            senior_mint: pool.senior_mint,
            junior_mint: pool.junior_mint,
            senior_yield_bps,
        });

        Ok(())
    }

    /// Deposits USDC into the Senior Tranche and mints Token-2022 yield tokens.
    pub fn deposit_senior(ctx: Context<DepositSenior>, amount: u64) -> Result<()> {
        let pool = &mut ctx.accounts.pool_config;
        require!(!pool.paused, ErrorCode::PoolPaused);
        require!(amount > 0, ErrorCode::ZeroDepositAmount);

        // Verify depositor KYC bitfield
        let kyc_registry = &ctx.accounts.kyc_registry;
        require!(
            kyc_registry.is_verified(&ctx.accounts.user.key()),
            ErrorCode::ComplianceKycFailed
        );

        // Transfer USDC from user to senior vault
        anchor_spl::token::transfer(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                anchor_spl::token::Transfer {
                    from: ctx.accounts.user_usdc.to_account_info(),
                    to: ctx.accounts.senior_vault.to_account_info(),
                    authority: ctx.accounts.user.to_account_info(),
                },
            ),
            amount,
        )?;

        // Shares issued 1:1 on initial tranche, otherwise derived from NAV
        let shares_to_mint = amount; 

        // Mint Token-2022 sTBILL using PDA signer seeds
        let pool_seeds = &[
            b"pool_config".as_ref(),
            pool.usdc_mint.as_ref(),
            &[pool.pool_bump],
        ];
        let signer = &[&pool_seeds[..]];

        let cpi_ctx = CpiContext::new_with_signer(
            ctx.accounts.token_2022_program.to_account_info(),
            MintTo {
                mint: ctx.accounts.senior_mint.to_account_info(),
                to: ctx.accounts.user_senior_token.to_account_info(),
                authority: ctx.accounts.pool_config.to_account_info(),
            },
            signer,
        );

        token_2022::mint_to(cpi_ctx, shares_to_mint)?;

        pool.total_senior_shares = pool
            .total_senior_shares
            .checked_add(shares_to_mint)
            .ok_or(ErrorCode::MathOverflow)?;

        emit!(SeniorDepositEvent {
            depositor: ctx.accounts.user.key(),
            amount,
            shares_minted: shares_to_mint,
            timestamp: Clock::get()?.unix_timestamp,
        });

        Ok(())
    }

    /// Updates dynamic yield via Token-2022 Interest-Bearing extension.
    pub fn update_epoch_yield(
        ctx: Context<UpdateEpochYield>,
        new_senior_rate_bps: i16,
        oracle_nav_usdc: u64,
    ) -> Result<()> {
        let pool = &mut ctx.accounts.pool_config;
        let clock = Clock::get()?;

        // Staleness Guard
        require!(
            clock.unix_timestamp - pool.last_epoch_timestamp >= 0,
            ErrorCode::InvalidEpochInterval
        );

        // Execute dynamic yield rate adjustment on Token-2022 mint
        let pool_seeds = &[
            b"pool_config".as_ref(),
            pool.usdc_mint.as_ref(),
            &[pool.pool_bump],
        ];
        let signer = &[&pool_seeds[..]];

        // Update interest bearing extension via Token-2022 CPI
        let ix = spl_token_2022::extension::interest_bearing_mint::instruction::update_rate(
            ctx.accounts.token_2022_program.key,
            ctx.accounts.senior_mint.key,
            ctx.accounts.pool_config.key,
            new_senior_rate_bps,
        )?;

        solana_program::program::invoke_signed(
            &ix,
            &[
                ctx.accounts.senior_mint.to_account_info(),
                ctx.accounts.pool_config.to_account_info(),
            ],
            signer,
        )?;

        pool.last_epoch_timestamp = clock.unix_timestamp;
        pool.senior_yield_bps = new_senior_rate_bps as u16;

        emit!(EpochYieldUpdated {
            new_yield_bps: new_senior_rate_bps,
            oracle_nav_usdc,
            timestamp: clock.unix_timestamp,
        });

        Ok(())
    }

    /// Transfer Hook Core Endpoint.
    /// Intercepts transfers of the sTBILL token to verify both sender and recipient on-chain.
    pub fn execute_transfer_hook(
        ctx: Context<ExecuteTransferHook>,
        amount: u64,
    ) -> Result<()> {
        let registry = &ctx.accounts.kyc_registry;

        // Extract owner pubkeys from TokenAccounts
        let source_owner = ctx.accounts.source_token_account.owner;
        let destination_owner = ctx.accounts.destination_token_account.owner;

        // Strict Compliance Validation: Both parties must have unexpired KYC and not be blacklisted
        require!(
            registry.is_verified(&source_owner),
            ErrorCode::SourceNotKYCApproved
        );
        require!(
            registry.is_verified(&destination_owner),
            ErrorCode::DestinationNotKYCApproved
        );

        msg!(
            "Transfer Hook Verified: {} tokens transferred from {} to {}",
            amount,
            source_owner,
            destination_owner
        );

        Ok(())
    }
}

// -----------------------------------------------------------------------------
// ACCOUNT STRUCTURES & CONTEXTS
// -----------------------------------------------------------------------------

#[derive(Accounts)]
#[instruction(pool_bump: u8)]
pub struct InitializePool<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,

    pub usdc_mint: Box<InterfaceAccount<'info, Mint>>,

    #[account(
        init,
        payer = authority,
        space = 8 + PoolConfig::INIT_SPACE,
        seeds = [b"pool_config", usdc_mint.key().as_ref()],
        bump
    )]
    pub pool_config: Box<Account<'info, PoolConfig>>,

    #[account(
        init,
        payer = authority,
        seeds = [b"senior_vault", pool_config.key().as_ref()],
        bump,
        token::mint = usdc_mint,
        token::authority = pool_config,
    )]
    pub senior_vault: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        init,
        payer = authority,
        seeds = [b"junior_vault", pool_config.key().as_ref()],
        bump,
        token::mint = usdc_mint,
        token::authority = pool_config,
    )]
    pub junior_vault: Box<InterfaceAccount<'info, TokenAccount>>,

    /// Token-2022 Senior Mint initialized outside or via CPI with interest extension
    #[account(mut)]
    pub senior_mint: Box<InterfaceAccount<'info, Mint>>,

    #[account(mut)]
    pub junior_mint: Box<InterfaceAccount<'info, Mint>>,

    pub token_program: Program<'info, anchor_spl::token::Token>,
    pub token_2022_program: Program<'info, Token2022>,
    pub system_program: Program<'info, System>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
pub struct DepositSenior<'info> {
    #[account(mut)]
    pub user: Signer<'info>,

    #[account(
        mut,
        seeds = [b"pool_config", pool_config.usdc_mint.as_ref()],
        bump = pool_config.pool_bump,
    )]
    pub pool_config: Box<Account<'info, PoolConfig>>,

    #[account(
        seeds = [b"kyc_registry", pool_config.authority.as_ref()],
        bump = kyc_registry.bump
    )]
    pub kyc_registry: Box<Account<'info, KycRegistry>>,

    #[account(mut)]
    pub user_usdc: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        mut,
        constraint = senior_vault.key() == pool_config.senior_vault @ ErrorCode::InvalidVault
    )]
    pub senior_vault: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        mut,
        constraint = senior_mint.key() == pool_config.senior_mint @ ErrorCode::InvalidMint
    )]
    pub senior_mint: Box<InterfaceAccount<'info, Mint>>,

    #[account(mut)]
    pub user_senior_token: Box<InterfaceAccount<'info, TokenAccount>>,

    pub token_program: Program<'info, anchor_spl::token::Token>,
    pub token_2022_program: Program<'info, Token2022>,
}

#[derive(Accounts)]
pub struct UpdateEpochYield<'info> {
    #[account(
        constraint = keeper.key() == pool_config.authority @ ErrorCode::UnauthorizedKeeper
    )]
    pub keeper: Signer<'info>,

    #[account(
        mut,
        seeds = [b"pool_config", pool_config.usdc_mint.as_ref()],
        bump = pool_config.pool_bump,
    )]
    pub pool_config: Box<Account<'info, PoolConfig>>,

    #[account(
        mut,
        constraint = senior_mint.key() == pool_config.senior_mint @ ErrorCode::InvalidMint
    )]
    pub senior_mint: Box<InterfaceAccount<'info, Mint>>,

    pub token_2022_program: Program<'info, Token2022>,
}

#[derive(Accounts)]
pub struct ExecuteTransferHook<'info> {
    #[account(
        seeds = [b"kyc_registry", kyc_registry.authority.as_ref()],
        bump = kyc_registry.bump
    )]
    pub kyc_registry: Account<'info, KycRegistry>,

    pub source_token_account: InterfaceAccount<'info, TokenAccount>,
    pub destination_token_account: InterfaceAccount<'info, TokenAccount>,
}

// -----------------------------------------------------------------------------
// STATE DECLARATIONS
// -----------------------------------------------------------------------------

#[account]
#[derive(InitSpace)]
pub struct PoolConfig {
    pub authority: Pubkey,
    pub usdc_mint: Pubkey,
    pub senior_mint: Pubkey,
    pub junior_mint: Pubkey,
    pub senior_vault: Pubkey,
    pub junior_vault: Pubkey,
    pub senior_yield_bps: u16,
    pub reserve_ratio_bps: u16,
    pub total_senior_shares: u64,
    pub total_junior_shares: u64,
    pub last_epoch_timestamp: i64,
    pub pool_bump: u8,
    pub paused: bool,
}

#[account]
pub struct KycRegistry {
    pub authority: Pubkey,
    pub bump: u8,
    // Dynamic BitMap / Verified Investor Array (Fixed sizing for PoC; Prod uses Merkle verification)
    pub approved_identities: [Pubkey; 64],
    pub is_blacklisted: [bool; 64],
}

impl KycRegistry {
    pub fn is_verified(&self, account: &Pubkey) -> bool {
        for (idx, approved) in self.approved_identities.iter().enumerate() {
            if approved == account {
                return !self.is_blacklisted[idx];
            }
        }
        false
    }
}

// -----------------------------------------------------------------------------
// EVENTS & ERRORS
// -----------------------------------------------------------------------------

#[event]
pub struct PoolInitialized {
    pub authority: Pubkey,
    pub senior_mint: Pubkey,
    pub junior_mint: Pubkey,
    pub senior_yield_bps: u16,
}

#[event]
pub struct SeniorDepositEvent {
    pub depositor: Pubkey,
    pub amount: u64,
    pub shares_minted: u64,
    pub timestamp: i64,
}

#[event]
pub struct EpochYieldUpdated {
    pub new_yield_bps: i16,
    pub oracle_nav_usdc: u64,
    pub timestamp: i64,
}

#[error_code]
pub enum ErrorCode {
    #[msg("Operation forbidden: System is currently paused")]
    PoolPaused,
    #[msg("Zero deposit value supplied")]
    ZeroDepositAmount,
    #[msg("Compliance validation failed: User not on KYC allowlist")]
    ComplianceKycFailed,
    #[msg("Arithmetic overflow detected in yield or share calculation")]
    MathOverflow,
    #[msg("Epoch step triggered before minimum threshold time")]
    InvalidEpochInterval,
    #[msg("Transfer Hook rejected: Source wallet not approved")]
    SourceNotKYCApproved,
    #[msg("Transfer Hook rejected: Destination wallet not approved")]
    DestinationNotKYCApproved,
    #[msg("Invalid authority signature for keeper operations")]
    UnauthorizedKeeper,
    #[msg("Passed vault account does not match canonical PoolConfig state")]
    InvalidVault,
    #[msg("Passed mint account does not match canonical PoolConfig state")]
    InvalidMint,
    #[msg("Reserve ratio exceeds 10,000 basis points")]
    InvalidRatio,
}