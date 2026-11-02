use anchor_lang::{
    prelude::*,
    system_program::{create_account, CreateAccount},
};
use anchor_spl::token_2022::Token2022;
use spl_tlv_account_resolution::{account::ExtraAccountMeta, seeds::Seed, state::ExtraAccountMetaList};
use spl_transfer_hook_interface::instruction::TransferHookInstruction;

declare_id!("Hook111111111111111111111111111111111111111");

pub const EXTRA_ACCOUNT_METAS_SEED: &[u8] = b"extra-account-metas";
pub const COMPLIANCE_SEED: &[u8] = b"compliance";
pub const POOL_SEED: &[u8] = b"debt-pool";

#[program]
pub mod rwa_transfer_hook {
    use super::*;

    /// Initializes a new Institutional Private Debt Asset Pool
    pub fn initialize_pool(
        ctx: Context<InitializePool>,
        pool_id: String,
        pool_name: String,
        coupon_bps: u16,
        maturity_timestamp: i64,
    ) -> Result<()> {
        require!(pool_id.len() <= 32, RwaError::IdentifierTooLong);
        require!(pool_name.len() <= 64, RwaError::IdentifierTooLong);
        require!(maturity_timestamp > Clock::get()?.unix_timestamp, RwaError::InvalidMaturity);

        let pool = &mut ctx.accounts.pool;
        pool.admin = ctx.accounts.admin.key();
        pool.mint = ctx.accounts.mint.key();
        pool.pool_id = pool_id;
        pool.pool_name = pool_name;
        pool.coupon_bps = coupon_bps;
        pool.maturity_timestamp = maturity_timestamp;
        pool.total_debt_issued = 0;
        pool.is_frozen = false;
        pool.bump = ctx.bumps.pool;

        emit!(PoolInitializedEvent {
            admin: pool.admin,
            mint: pool.mint,
            pool_id: pool.pool_id.clone(),
            coupon_bps,
            maturity_timestamp,
        });

        Ok(())
    }

    /// Sets up the ExtraAccountMetas account required by SPL Token-2022 transfer hook interface
    pub fn initialize_extra_account_meta_list(
        ctx: Context<InitializeExtraAccountMetaList>,
    ) -> Result<()> {
        let account_metas = vec![
            // Index 4: Extra Account: Compliance Registry for the Sender
            ExtraAccountMeta::new_with_seeds(
                &[
                    Seed::Literal {
                        bytes: COMPLIANCE_SEED.to_vec(),
                    },
                    Seed::AccountKey { index: 0 }, // Source token account
                ],
                false, // is_signer
                false, // is_writable
            ).map_err(|_| RwaError::AccountResolutionError)?,
            // Index 5: Extra Account: Compliance Registry for the Recipient
            ExtraAccountMeta::new_with_seeds(
                &[
                    Seed::Literal {
                        bytes: COMPLIANCE_SEED.to_vec(),
                    },
                    Seed::AccountKey { index: 2 }, // Destination token account
                ],
                false, // is_signer
                false, // is_writable
            ).map_err(|_| RwaError::AccountResolutionError)?,
            // Index 6: Extra Account: Pool PDA to verify freeze status
            ExtraAccountMeta::new_with_seeds(
                &[
                    Seed::Literal {
                        bytes: POOL_SEED.to_vec(),
                    },
                    Seed::AccountKey { index: 1 }, // Mint key
                ],
                false, // is_signer
                false, // is_writable
            ).map_err(|_| RwaError::AccountResolutionError)?,
        ];

        let account_size = ExtraAccountMetaList::size_of(account_metas.len())
            .map_err(|_| RwaError::AccountResolutionError)?;
        let lamports = Rent::get()?.minimum_balance(account_size);

        let mint_key = ctx.accounts.mint.key();
        let signer_seeds: &[&[&[u8]]] = &[&[
            EXTRA_ACCOUNT_METAS_SEED,
            mint_key.as_ref(),
            &[ctx.bumps.extra_account_metas],
        ]];

        create_account(
            CpiContext::new_with_signer(
                ctx.accounts.system_program.to_account_info(),
                CreateAccount {
                    from: ctx.accounts.admin.to_account_info(),
                    to: ctx.accounts.extra_account_metas.to_account_info(),
                },
                signer_seeds,
            ),
            lamports,
            account_size as u64,
            ctx.program_id,
        )?;

        let mut data = ctx.accounts.extra_account_metas.try_borrow_mut_data()?;
        ExtraAccountMetaList::init::<&[u8]>(&mut data, &account_metas)
            .map_err(|_| RwaError::AccountResolutionError)?;

        Ok(())
    }

    /// Whitelist and update institutional verification data for an investor account
    pub fn update_investor_compliance(
        ctx: Context<UpdateInvestorCompliance>,
        accredited_status: bool,
        country_code: u16,
        kyc_expiry: i64,
        is_blacklisted: bool,
    ) -> Result<()> {
        let compliance = &mut ctx.accounts.compliance_entry;
        compliance.token_account = ctx.accounts.investor_token_account.key();
        compliance.pool = ctx.accounts.pool.key();
        compliance.accredited_status = accredited_status;
        compliance.country_code = country_code;
        compliance.kyc_expiry = kyc_expiry;
        compliance.is_blacklisted = is_blacklisted;
        compliance.bump = ctx.bumps.compliance_entry;

        emit!(InvestorComplianceUpdatedEvent {
            token_account: compliance.token_account,
            accredited_status,
            country_code,
            kyc_expiry,
            is_blacklisted,
        });

        Ok(())
    }

    /// SPL Token-2022 Transfer Hook Core Entry Point
    /// Executed deterministically via CPI by the Token-2022 program before any balance mutation
    pub fn transfer_hook(ctx: Context<TransferHookContext>, amount: u64) -> Result<()> {
        let pool = &ctx.accounts.pool;
        require!(!pool.is_frozen, RwaError::PoolIsFrozen);

        let current_time = Clock::get()?.unix_timestamp;

        // Check Sender Compliance
        let source_compliance = &ctx.accounts.source_compliance;
        require!(
            source_compliance.accredited_status,
            RwaError::SourceNotAccredited
        );
        require!(
            !source_compliance.is_blacklisted,
            RwaError::SourceBlacklisted
        );
        require!(
            source_compliance.kyc_expiry > current_time,
            RwaError::SourceKycExpired
        );

        // Check Recipient Compliance
        let destination_compliance = &ctx.accounts.destination_compliance;
        require!(
            destination_compliance.accredited_status,
            RwaError::DestinationNotAccredited
        );
        require!(
            !destination_compliance.is_blacklisted,
            RwaError::DestinationBlacklisted
        );
        require!(
            destination_compliance.kyc_expiry > current_time,
            RwaError::DestinationKycExpired
        );

        emit!(TransferComplianceVerifiedEvent {
            source: ctx.accounts.source_account.key(),
            destination: ctx.accounts.destination_account.key(),
            amount,
            timestamp: current_time,
        });

        Ok(())
    }

    /// Admin Emergency Controls: Circuit Breaker
    pub fn set_pool_freeze(ctx: Context<SetPoolFreeze>, freeze: bool) -> Result<()> {
        let pool = &mut ctx.accounts.pool;
        pool.is_frozen = freeze;
        emit!(PoolFreezeToggledEvent {
            pool: pool.key(),
            is_frozen: freeze,
        });
        Ok(())
    }

    /// SPL-Transfer-Hook fallback processor
    pub fn fallback<'info>(
        program_id: &Pubkey,
        accounts: &'info [AccountInfo<'info>],
        data: &[u8],
    ) -> Result<()> {
        let instruction = TransferHookInstruction::unpack(data)
            .map_err(|_| ProgramError::InvalidInstructionData)?;

        match instruction {
            TransferHookInstruction::Execute { amount } => {
                let amount_bytes = amount.to_le_bytes();
                __private::__global::transfer_hook(program_id, accounts, &amount_bytes)
            }
            _ => Err(ProgramError::InvalidInstructionData.into()),
        }
    }
}

// ------------------------------------------------------------------------
// State Accounts
// ------------------------------------------------------------------------

#[account]
#[derive(Default)]
pub struct DebtPool {
    pub admin: Pubkey,            // 32
    pub mint: Pubkey,             // 32
    pub pool_id: String,          // 4 + 32
    pub pool_name: String,        // 4 + 64
    pub coupon_bps: u16,          // 2 (e.g. 850 = 8.50% APR)
    pub maturity_timestamp: i64,  // 8
    pub total_debt_issued: u64,   // 8
    pub is_frozen: bool,          // 1
    pub bump: u8,                 // 1
}

impl DebtPool {
    pub const LEN: usize = 8 + 32 + 32 + (4 + 32) + (4 + 64) + 2 + 8 + 8 + 1 + 1;
}

#[account]
#[derive(Default)]
pub struct InvestorCompliance {
    pub token_account: Pubkey,    // 32
    pub pool: Pubkey,             // 32
    pub accredited_status: bool,  // 1
    pub country_code: u16,        // 2 (ISO 3166-1 numeric, e.g., 840 = US)
    pub kyc_expiry: i64,          // 8
    pub is_blacklisted: bool,     // 1
    pub bump: u8,                 // 1
}

impl InvestorCompliance {
    pub const LEN: usize = 8 + 32 + 32 + 1 + 2 + 8 + 1 + 1;
}

// ------------------------------------------------------------------------
// Instruction Contexts
// ------------------------------------------------------------------------

#[derive(Accounts)]
#[instruction(pool_id: String)]
pub struct InitializePool<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,

    /// CHECK: The SPL Token-2022 Mint initialized with transfer hook extension
    pub mint: AccountInfo<'info>,

    #[account(
        init,
        payer = admin,
        space = DebtPool::LEN,
        seeds = [POOL_SEED, mint.key().as_ref()],
        bump
    )]
    pub pool: Account<'info, DebtPool>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct InitializeExtraAccountMetaList<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,

    /// CHECK: The SPL Token-2022 Mint
    pub mint: AccountInfo<'info>,

    /// CHECK: ExtraAccountMetaList Account to be created
    #[account(
        mut,
        seeds = [EXTRA_ACCOUNT_METAS_SEED, mint.key().as_ref()],
        bump
    )]
    pub extra_account_metas: AccountInfo<'info>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct UpdateInvestorCompliance<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,

    #[account(
        has_one = admin @ RwaError::UnauthorizedAdmin
    )]
    pub pool: Account<'info, DebtPool>,

    /// CHECK: The user's SPL token account address being validated
    pub investor_token_account: AccountInfo<'info>,

    #[account(
        init_if_needed,
        payer = admin,
        space = InvestorCompliance::LEN,
        seeds = [COMPLIANCE_SEED, investor_token_account.key().as_ref()],
        bump
    )]
    pub compliance_entry: Account<'info, InvestorCompliance>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct TransferHookContext<'info> {
    /// CHECK: Source token account
    pub source_account: AccountInfo<'info>,

    /// CHECK: Mint account
    pub mint: AccountInfo<'info>,

    /// CHECK: Destination token account
    pub destination_account: AccountInfo<'info>,

    /// CHECK: Authority/Owner of source token account
    pub owner: AccountInfo<'info>,

    /// CHECK: ExtraAccountMetas account (validated by Token-2022 runtime)
    #[account(
        seeds = [EXTRA_ACCOUNT_METAS_SEED, mint.key().as_ref()],
        bump
    )]
    pub extra_account_metas: AccountInfo<'info>,

    #[account(
        seeds = [COMPLIANCE_SEED, source_account.key().as_ref()],
        bump = source_compliance.bump
    )]
    pub source_compliance: Account<'info, InvestorCompliance>,

    #[account(
        seeds = [COMPLIANCE_SEED, destination_account.key().as_ref()],
        bump = destination_compliance.bump
    )]
    pub destination_compliance: Account<'info, InvestorCompliance>,

    #[account(
        seeds = [POOL_SEED, mint.key().as_ref()],
        bump = pool.bump
    )]
    pub pool: Account<'info, DebtPool>,
}

#[derive(Accounts)]
pub struct SetPoolFreeze<'info> {
    #[account(
        mut,
        has_one = admin @ RwaError::UnauthorizedAdmin
    )]
    pub pool: Account<'info, DebtPool>,
    pub admin: Signer<'info>,
}

// ------------------------------------------------------------------------
// Events & Errors
// ------------------------------------------------------------------------

#[event]
pub struct PoolInitializedEvent {
    pub admin: Pubkey,
    pub mint: Pubkey,
    pub pool_id: String,
    pub coupon_bps: u16,
    pub maturity_timestamp: i64,
}

#[event]
pub struct InvestorComplianceUpdatedEvent {
    pub token_account: Pubkey,
    pub accredited_status: bool,
    pub country_code: u16,
    pub kyc_expiry: i64,
    pub is_blacklisted: bool,
}

#[event]
pub struct TransferComplianceVerifiedEvent {
    pub source: Pubkey,
    pub destination: Pubkey,
    pub amount: u64,
    pub timestamp: i64,
}

#[event]
pub struct PoolFreezeToggledEvent {
    pub pool: Pubkey,
    pub is_frozen: bool,
}

#[error_code]
pub enum RwaError {
    #[msg("Identifier exceeds allocated length limits.")]
    IdentifierTooLong,
    #[msg("Maturity timestamp must be strictly in the future.")]
    InvalidMaturity,
    #[msg("Failed during TLV account resolution generation.")]
    AccountResolutionError,
    #[msg("Caller is unauthorized to modify pool settings.")]
    UnauthorizedAdmin,
    #[msg("Asset Pool is frozen by institutional administrator.")]
    PoolIsFrozen,
    #[msg("Transfer Rejected: Source investor account is not accredited.")]
    SourceNotAccredited,
    #[msg("Transfer Rejected: Source investor is flagged on sanction/blacklist.")]
    SourceBlacklisted,
    #[msg("Transfer Rejected: Source investor KYC attestation has expired.")]
    SourceKycExpired,
    #[msg("Transfer Rejected: Destination investor account is not accredited.")]
    DestinationNotAccredited,
    #[msg("Transfer Rejected: Destination investor is flagged on sanction/blacklist.")]
    DestinationBlacklisted,
    #[msg("Transfer Rejected: Destination investor KYC attestation has expired.")]
    DestinationKycExpired,
}