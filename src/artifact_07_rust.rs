use anchor_lang::prelude::*;
use spl_transfer_hook_interface::instruction::TransferHookInstruction;

declare_id!("Hook111111111111111111111111111111111111111");

pub const REGISTRY_SEED: &[u8] = b"investor_compliance_registry";

#[program]
pub mod compliance_transfer_hook {
    use super::*;

    pub fn initialize_compliance_entry(
        ctx: Context<InitializeComplianceEntry>,
        is_accredited: bool,
        is_sanctioned: bool,
    ) -> Result<()> {
        let registry = &mut ctx.accounts.registry;
        registry.investor = ctx.accounts.investor.key();
        registry.is_accredited = is_accredited;
        registry.is_sanctioned = is_sanctioned;
        registry.bump = ctx.bumps.registry;
        Ok(())
    }

    /// Invoked atomically by SPL Token-2022 on every transfer!
    pub fn execute(ctx: Context<ExecuteTransferHook>, amount: u64) -> Result<()> {
        let sender_registry = &ctx.accounts.sender_registry;
        let receiver_registry = &ctx.accounts.receiver_registry;

        // Block all sanctioned entities
        require!(!sender_registry.is_sanctioned, ComplianceError::SanctionedEntity);
        require!(!receiver_registry.is_sanctioned, ComplianceError::SanctionedEntity);

        // Require accredited status for secondary market trading of private debt tranche notes
        require!(receiver_registry.is_accredited, ComplianceError::NonAccreditedInvestor);

        msg!("Compliance check passed: Transfer of {} units permitted.", amount);
        Ok(())
    }

    /// Entry point conforming to spl-transfer-hook-interface
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
                __private::__global::execute(program_id, accounts, &amount_bytes)
            }
            _ => Err(ProgramError::InvalidInstructionData.into()),
        }
    }
}

#[derive(Accounts)]
pub struct InitializeComplianceEntry<'info> {
    #[account(
        init,
        payer = authority,
        space = 8 + ComplianceRegistry::INIT_SPACE,
        seeds = [REGISTRY_SEED, investor.key().as_ref()],
        bump
    )]
    pub registry: Account<'info, ComplianceRegistry>,

    pub investor: SystemAccount<'info>,

    #[account(mut)]
    pub authority: Signer<'info>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct ExecuteTransferHook<'info> {
    #[account(
        seeds = [REGISTRY_SEED, source_owner.key().as_ref()],
        bump = sender_registry.bump
    )]
    pub sender_registry: Account<'info, ComplianceRegistry>,

    #[account(
        seeds = [REGISTRY_SEED, destination_owner.key().as_ref()],
        bump = receiver_registry.bump
    )]
    pub receiver_registry: Account<'info, ComplianceRegistry>,

    pub source_owner: SystemAccount<'info>,
    pub destination_owner: SystemAccount<'info>,
}

#[account]
#[derive(InitSpace)]
pub struct ComplianceRegistry {
    pub investor: Pubkey,
    pub is_accredited: bool,
    pub is_sanctioned: bool,
    pub bump: u8,
}

#[error_code]
pub enum ComplianceError {
    #[msg("Address detected on global sanctions watchlist. Transfer blocked.")]
    SanctionedEntity,
    #[msg("Recipient is not registered as an accredited investor.")]
    NonAccreditedInvestor,
}