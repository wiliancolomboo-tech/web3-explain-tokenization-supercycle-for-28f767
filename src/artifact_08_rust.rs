use anchor_lang::prelude::*;
use crate::state::ComplianceRecord;
use crate::error::CreditPoolError;

#[derive(Accounts)]
#[instruction(investor_wallet: Pubkey)]
pub struct RegisterCompliance<'info> {
    #[account(mut)]
    pub compliance_officer: Signer<'info>,

    #[account(
        init_if_needed,
        payer = compliance_officer,
        space = ComplianceRecord::LEN,
        seeds = [b"compliance_record", investor_wallet.as_ref()],
        bump
    )]
    pub compliance_record: Account<'info, ComplianceRecord>,

    pub system_program: Program<'info, System>,
}

pub fn handler(
    ctx: Context<RegisterCompliance>,
    investor_wallet: Pubkey,
    kyc_expiry: i64,
    jurisdiction_flags: u32,
    is_sanctioned: bool,
) -> Result<()> {
    let clock = Clock::get()?;
    require!(kyc_expiry > clock.unix_timestamp, CreditPoolError::KycExpired);

    let record = &mut ctx.accounts.compliance_record;
    record.investor = investor_wallet;
    record.kyc_expiry = kyc_expiry;
    record.jurisdiction_flags = jurisdiction_flags;
    record.is_sanctioned = is_sanctioned;
    record.bump = ctx.bumps.compliance_record;

    msg!(
        "Compliance verified for LP {}. Expiry: {}, Sanctioned: {}",
        investor_wallet,
        kyc_expiry,
        is_sanctioned
    );
    Ok(())
}