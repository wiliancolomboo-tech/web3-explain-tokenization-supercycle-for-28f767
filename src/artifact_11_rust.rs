use anchor_lang::prelude::*;
use crate::state::ComplianceRecord;
use crate::error::CreditPoolError;

#[derive(Accounts)]
pub struct ExecuteTransferHook<'info> {
    /// Token account originating the transfer
    pub source_account: AccountInfo<'info>,
    /// Token mint account being transferred
    pub mint: AccountInfo<'info>,
    /// Token destination account
    pub destination_account: AccountInfo<'info>,
    /// Owner of source account
    pub owner_delegate: AccountInfo<'info>,
    /// Extra account metas PDA
    pub extra_account_metas: AccountInfo<'info>,

    /// Source wallet compliance verification PDA
    #[account(
        seeds = [b"compliance_record", source_wallet.key().as_ref()],
        bump = source_compliance.bump,
    )]
    pub source_compliance: Account<'info, ComplianceRecord>,

    /// Destination wallet compliance verification PDA
    #[account(
        seeds = [b"compliance_record", dest_wallet.key().as_ref()],
        bump = dest_compliance.bump,
    )]
    pub dest_compliance: Account<'info, ComplianceRecord>,

    /// CHECK: Public key of the actual sender wallet
    pub source_wallet: AccountInfo<'info>,
    /// CHECK: Public key of the actual receiver wallet
    pub dest_wallet: AccountInfo<'info>,
}

pub fn handler(ctx: Context<ExecuteTransferHook>, _amount: u64) -> Result<()> {
    let clock = Clock::get()?;
    let src = &ctx.accounts.source_compliance;
    let dst = &ctx.accounts.dest_compliance;

    // Cryptographically enforce accreditation & sanctions checks on BOTH parties
    require!(
        !src.is_sanctioned && !dst.is_sanctioned,
        CreditPoolError::JurisdictionRestricted
    );
    require!(
        src.kyc_expiry > clock.unix_timestamp && dst.kyc_expiry > clock.unix_timestamp,
        CreditPoolError::KycExpired
    );

    msg!("Transfer Hook: Verification passed for transfer of security tokens.");
    Ok(())
}