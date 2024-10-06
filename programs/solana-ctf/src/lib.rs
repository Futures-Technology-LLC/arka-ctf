use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{self, mint_to, Mint, MintTo, Token, TokenAccount},
};
use spl_token::instruction::AuthorityType;
use std::slice::Iter;

declare_id!("EcVyZwmzssLbWBnnf7gSqVkZmc96iTNaYe4jQTrfHDLA");

#[program]
pub mod solana_ctf {
    use super::*;

    pub fn mint_tokens(ctx: Context<MintTokens>, params: MintTokenParams) -> Result<()> {
        // Validate that the price is between (0-1 dollar)
        let event_total_price = ctx.accounts.event_data.event_total_price;
        if params.token_price > event_total_price {
            return Err(BuyTokenError::InvalidPrice.into());
        }

        let bump = ctx.bumps.delegate.to_be_bytes();
        let event_id_bytes = params.event_id.to_le_bytes();
        let seeds = &[b"usdc_eid_", event_id_bytes.as_ref(), bump.as_ref()];
        let signer_seeds = [&seeds[..]];

        /* Debit the USDC from user account to Arka account */
        let usdc_amount = params.token_price * params.quantity;
        msg!("Total amount of USDC to deduct: {:?}", usdc_amount);

        let cpi_accounts = token::Transfer {
            from: ctx.accounts.user_usdc_token_account.to_account_info(),
            to: ctx.accounts.arka_usdc_token_account.to_account_info(),
            authority: ctx.accounts.delegate.to_account_info(),
        };

        let cpi_context = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            cpi_accounts,
            &signer_seeds,
        );

        token::transfer(cpi_context, usdc_amount)?;

        /* Mint Arka token into user account */
        let event_id = params.event_id.to_le_bytes();
        let token_price = params.token_price.to_le_bytes();
        let seeds = &[
            b"eid_",
            event_id.as_ref(),
            b"_tt_",
            &[params.token_type.clone() as u8],
            b"_tp_",
            token_price.as_ref(),
            &[ctx.bumps.arka_mint],
        ];
        let signer = [&seeds[..]];

        mint_to(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                MintTo {
                    authority: ctx.accounts.arka_mint.to_account_info(),
                    to: ctx.accounts.user_arka_token_account.to_account_info(),
                    mint: ctx.accounts.arka_mint.to_account_info(),
                },
                &signer,
            ),
            params.quantity,
        )?;

        Ok(())
    }

    pub fn initialize_event(ctx: Context<InitializeEvent>, data: InitEventParams) -> Result<()> {
        if data.commission_rate > 100 {
            return Err(InitializeEventError::InvalidCommissionRate.into());
        }

        ctx.accounts.event_data.comission_rate = data.commission_rate;
        ctx.accounts.event_data.event_id = data.event_id;
        ctx.accounts.event_data.outcome = EventOutcome::Null;
        ctx.accounts.event_data.is_outcome_set = false;
        ctx.accounts.event_data.event_total_price = data.event_total_price;

        msg!(
            "Event created on chain with event_id={:?}, event_price={:?} and commission_rate={:?}",
            data.event_id,
            data.event_total_price,
            data.commission_rate
        );

        let bump = ctx.bumps.escrow_account.to_be_bytes();
        let event_id_bytes = data.event_id.to_le_bytes();
        let seeds = &[b"usdc_eid_", event_id_bytes.as_ref(), bump.as_ref()];
        let signer_seeds = [&seeds[..]];

        token::set_authority(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                token::SetAuthority {
                    account_or_mint: ctx.accounts.escrow_account.to_account_info(),
                    current_authority: ctx.accounts.delegate.to_account_info(),
                },
                &signer_seeds,
            ),
            AuthorityType::AccountOwner,
            Some(ctx.accounts.delegate.key()), // Set the PDA as the new owner
        )?;

        Ok(())
    }

    pub fn update_outcome(ctx: Context<UpdateOutcome>, data: EventOutcome) -> Result<()> {
        if data == EventOutcome::Null {
            return Err(UpdateOutcomeError::InvalidOutcomeState.into());
        }

        if ctx.accounts.event_data.is_outcome_set {
            return Err(UpdateOutcomeError::OutcomeAlreadyUpdated.into());
        }

        ctx.accounts.event_data.outcome = data;
        ctx.accounts.event_data.is_outcome_set = true;

        msg!(
            "Updating for event_id={:?} event_outcome={:?}",
            ctx.accounts.event_data.event_id,
            ctx.accounts.event_data.outcome
        );

        Ok(())
    }

    pub fn create_mint(_ctx: Context<CreateMintData>, params: CreateMintParams) -> Result<()> {
        msg!(
            "Creating mint from event_id={:?}, token_type={:?}, token_price={:?}",
            params.event_id,
            params.token_type,
            params.token_price
        );
        Ok(())
    }

    pub fn burn_tokens(ctx: Context<BurnTokens>, params: BurnTokenParams) -> Result<()> {
        // Validate that the price is between (0-1 dollar)
        let event_total_price = ctx.accounts.event_data.event_total_price;
        if params.token_price > event_total_price {
            return Err(SellTokenError::InvalidTokenPrice.into());
        }

        if params.selling_price == event_total_price {
            if !ctx.accounts.event_data.is_outcome_set {
                return Err(SellTokenError::EventNotFinished.into());
            }

            let outcome = ctx.accounts.event_data.outcome.clone() as u8 - 1;
            if outcome != params.token_type as u8 {
                return Err(SellTokenError::EventOutcomeMismatch.into());
            }
        }

        /* Debit the USDC from user account to Arka account */
        let purchase_price = params.token_price * params.quantity;
        let selling_price = params.selling_price * params.quantity;

        let mut amount_to_return = selling_price;
        let mut commission = 0;

        // User is making a profit, thus we need to deduct commission
        if selling_price > purchase_price {
            let commission_rate = ctx.accounts.event_data.comission_rate;
            let profit = selling_price - purchase_price;
            commission = (commission_rate * profit) / 100;
            amount_to_return = selling_price - commission;
        }

        msg!(
            "Purchase price: {:?}, Selling Price: {:?}, commission: {:?}",
            purchase_price,
            selling_price,
            commission,
        );

        let bump = ctx.bumps.delegate.to_be_bytes();
        let event_id = params.event_id.to_le_bytes();
        let seeds = &[b"usdc_eid_", event_id.as_ref(), bump.as_ref()];
        let signer_seeds = [&seeds[..]];

        if params.selling_price > 0 {
            // let program_pda = Pubkey::from_str("EMSsxw9k6kFPp2F4XMdp87q9NwDvAbkMYdRo9BzV1VbP");
            let cpi_accounts = token::Transfer {
                to: ctx.accounts.user_usdc_token_account.to_account_info(),
                from: ctx.accounts.arka_usdc_event_token_account.to_account_info(),
                authority: ctx.accounts.delegate.to_account_info(),
            };

            let cpi_context = CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                cpi_accounts,
                &signer_seeds,
            );

            token::transfer(cpi_context, amount_to_return)?;

            let cpi_accounts = token::Transfer {
                to: ctx.accounts.arka_usdc_token_account.to_account_info(),
                from: ctx.accounts.arka_usdc_event_token_account.to_account_info(),
                authority: ctx.accounts.delegate.to_account_info(),
            };

            let cpi_context = CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                cpi_accounts,
                &signer_seeds,
            );

            token::transfer(cpi_context, commission)?;
        }

        /* Burn Arka token from user account */
        let cpi_accounts = token::Burn {
            mint: ctx.accounts.arka_mint.to_account_info(),
            from: ctx.accounts.user_arka_token_account.to_account_info(),
            authority: ctx.accounts.authority.to_account_info(),
        };
        let cpi_program = ctx.accounts.token_program.to_account_info();
        let cpi_ctx = CpiContext::new(cpi_program, cpi_accounts);

        token::burn(cpi_ctx, params.quantity)?;

        Ok(())
    }
}

#[error_code]
pub enum BuyTokenError {
    #[msg("Price > 100")]
    InvalidPrice,
}

#[error_code]
pub enum SellTokenError {
    #[msg("Price > 100")]
    InvalidTokenPrice,
    #[msg("Price > 100")]
    InvalidSellingPrice,
    #[msg("Event has no outcome set yet!")]
    EventNotFinished,
    #[msg("Event outcome does not match price!")]
    EventOutcomeMismatch,
}

#[repr(u8)]
#[derive(
    Debug, PartialEq, Eq, Clone, AnchorSerialize, AnchorDeserialize, serde::Deserialize, Copy,
)]
pub enum TokenType {
    Yes = 0,
    No,
}

impl TokenType {
    pub fn iterator() -> Iter<'static, TokenType> {
        static TOKEN_TYPES: [TokenType; 2] = [TokenType::Yes, TokenType::No];
        TOKEN_TYPES.iter()
    }
}

#[derive(AnchorSerialize, AnchorDeserialize, Debug, Clone)]
pub struct CreateMintParams {
    pub event_id: u64,
    pub token_type: TokenType,
    pub token_price: u64,
    pub mint_authority: Pubkey,
}

#[derive(Accounts)]
#[instruction(params: CreateMintParams)]
pub struct CreateMintData<'info> {
    // constant 8 in space denotes the size of the discriminator
    #[account(
        init,
        payer = payer,
        seeds = [b"eid_", params.event_id.to_le_bytes().as_ref(), b"_tt_", &[params.token_type.clone() as u8], b"_tp_", params.token_price.to_le_bytes().as_ref()],
        bump,
        mint::decimals = 0,
        mint::authority = mint,
    )]
    pub mint: Account<'info, Mint>,
    #[account(mut)]
    pub payer: Signer<'info>,
    pub rent: Sysvar<'info, Rent>,
    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct UpdateOutcome<'info> {
    #[account(mut)]
    pub event_data: Account<'info, EventData>,
    pub user: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[error_code]
pub enum InitEventError {
    #[msg("Only 100 mints are supported as of now.")]
    MintPdaLimitExceeded,
    #[msg("Mint PDA vector len mismatch.")]
    MalformedPdaVector,
    #[msg("Price must be between 0 to 100")]
    InvalidPrice,
}

#[error_code]
pub enum UpdateOutcomeError {
    #[msg("Trying to set outcome to Null, not allowed.")]
    InvalidOutcomeState,
    #[msg("You are only allowed to update once.")]
    OutcomeAlreadyUpdated,
}

#[derive(AnchorSerialize, AnchorDeserialize, Debug, Clone)]
pub struct InitEventParams {
    pub event_id: u64,
    pub commission_rate: u64,
    pub event_total_price: u64,
}

#[repr(u8)]
#[derive(Default, Debug, PartialEq, Eq, Clone, AnchorSerialize, AnchorDeserialize)]
pub enum EventOutcome {
    #[default]
    Null = 0,
    Yes,
    No,
}

impl From<u8> for EventOutcome {
    fn from(value: u8) -> Self {
        match value {
            0 => EventOutcome::Null,
            1 => EventOutcome::Yes,
            2 => EventOutcome::No,
            _ => EventOutcome::Null, // Default case if value doesn't match
        }
    }
}

#[account]
pub struct EventData {
    pub event_id: u64,
    pub comission_rate: u64,
    pub outcome: EventOutcome,
    pub is_outcome_set: bool,
    pub event_total_price: u64,
}

impl EventData {
    pub const LEN: usize = std::mem::size_of::<EventData>();
}

#[error_code]
pub enum InitializeEventError {
    #[msg("Trying to set commission rate > 100")]
    InvalidCommissionRate,
}

#[derive(Accounts)]
#[instruction(params: InitEventParams)]
pub struct InitializeEvent<'info> {
    // constant 8 in space denotes the size of the discriminator
    #[account(
        init,
        payer = payer,
        space = 8 + EventData::LEN,
        seeds = [b"eid_", params.event_id.to_le_bytes().as_ref()],
        bump
    )]
    pub event_data: Account<'info, EventData>,
    pub usdc_mint: Account<'info, Mint>,
    #[account(
        init,
        seeds = [b"usdc_eid_", params.event_id.to_le_bytes().as_ref()],
        bump,
        payer = payer,
        token::mint = usdc_mint,
        token::authority = delegate,
    )]
    pub escrow_account: Account<'info, TokenAccount>,
    #[account(
        seeds = [b"usdc_eid_", params.event_id.to_le_bytes().as_ref()],
        bump,
    )]
    /// CHECK: This account is safe as it is used to set the delegate authority for the token account
    pub delegate: AccountInfo<'info>,
    #[account(mut)]
    pub payer: Signer<'info>,
    pub system_program: Program<'info, System>,
    pub rent: Sysvar<'info, Rent>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
}

// Token initialization params
#[derive(AnchorSerialize, AnchorDeserialize, Debug, Clone)]
pub struct MintTokenParams {
    pub token_type: TokenType,
    pub token_price: u64,
    pub event_id: u64,
    pub quantity: u64,
    pub user_id: u64,
}

#[derive(Accounts)]
#[instruction(params: MintTokenParams)]
pub struct MintTokens<'info> {
    #[account(
        mut,
        seeds = [b"eid_", params.event_id.to_le_bytes().as_ref(), b"_tt_", &[params.token_type.clone() as u8], b"_tp_", params.token_price.to_le_bytes().as_ref()],
        bump,
        mint::authority = arka_mint,
    )]
    pub arka_mint: Account<'info, Mint>,
    #[account(
        init_if_needed,
        seeds = [b"uid_", params.user_id.to_le_bytes().as_ref(), b"_eid_", params.event_id.to_le_bytes().as_ref(), b"_tt_", &[params.token_type.clone() as u8], b"_tp_", params.token_price.to_le_bytes().as_ref()],
        bump,
        payer = payer,
        token::mint = arka_mint,
        token::authority = payer,
    )]
    pub user_arka_token_account: Account<'info, TokenAccount>,
    #[account(mut)]
    pub user_usdc_token_account: Account<'info, TokenAccount>,
    #[account(mut)]
    pub arka_usdc_token_account: Account<'info, TokenAccount>,
    #[account(mut)]
    pub payer: Signer<'info>,
    pub rent: Sysvar<'info, Rent>,
    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    #[account(signer)]
    pub authority: Signer<'info>,
    /// CHECK: This account is safe as it is used to set the delegate authority for the token account
    #[account(
        seeds = [b"usdc_eid_", params.event_id.to_le_bytes().as_ref()],
        bump,
    )]
    pub delegate: AccountInfo<'info>,
    pub event_data: Account<'info, EventData>,
}

// Token initialization params
#[derive(AnchorSerialize, AnchorDeserialize, Debug, Clone, serde::Deserialize)]
pub struct BurnTokenParams {
    pub token_type: TokenType,
    pub token_price: u64,
    pub event_id: u64,
    pub quantity: u64,
    pub user_id: u64,
    pub selling_price: u64,
}

#[derive(Accounts)]
#[instruction(params: BurnTokenParams)]
pub struct BurnTokens<'info> {
    #[account(
        mut,
        seeds = [b"eid_", params.event_id.to_le_bytes().as_ref(), b"_tt_", &[params.token_type.clone() as u8], b"_tp_", params.token_price.to_le_bytes().as_ref()],
        bump,
        mint::authority = arka_mint,
    )]
    pub arka_mint: Account<'info, Mint>,
    #[account(
        init_if_needed,
        seeds = [b"uid_", params.user_id.to_le_bytes().as_ref(), b"_eid_", params.event_id.to_le_bytes().as_ref(), b"_tt_", &[params.token_type.clone() as u8], b"_tp_", params.token_price.to_le_bytes().as_ref()],
        bump,
        payer = payer,
        token::mint = arka_mint,
        token::authority = payer,
    )]
    pub user_arka_token_account: Account<'info, TokenAccount>,
    #[account(mut)]
    pub user_usdc_token_account: Account<'info, TokenAccount>,
    #[account(
        mut,
        seeds = [b"usdc_eid_", params.event_id.to_le_bytes().as_ref()],
        bump,
    )]
    pub arka_usdc_event_token_account: Account<'info, TokenAccount>,
    #[account(mut)]
    pub arka_usdc_token_account: Account<'info, TokenAccount>,
    #[account(mut, signer)]
    pub payer: Signer<'info>,
    pub rent: Sysvar<'info, Rent>,
    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    /// CHECK: This account is safe as it is used to set the delegate authority for the token account
    #[account(
        seeds = [b"usdc_eid_", params.event_id.to_le_bytes().as_ref()],
        bump,
    )]
    pub delegate: AccountInfo<'info>,
    #[account(signer)]
    pub authority: Signer<'info>,
    pub event_data: Account<'info, EventData>,
}
