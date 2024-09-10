use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{self, mint_to, Mint, MintTo, Token, TokenAccount},
};

declare_id!("EMSsxw9k6kFPp2F4XMdp87q9NwDvAbkMYdRo9BzV1VbP");

pub const USDC_DECIMAL: u64 = 100000;
pub const ONE_DOLLAR: u64 = USDC_DECIMAL * 10;

#[program]
pub mod solana_ctf {
    use super::*;

    pub fn settle_balance(ctx: Context<SettleBalance>, params: SettleBalanceParams) -> Result<()> {
        if !ctx.accounts.event_data.is_outcome_set {
            return Err(SettleBalanceError::EventNotFinished.into());
        }

        let mut selling_price = ONE_DOLLAR;
        let outcome = ctx.accounts.event_data.outcome.clone() as u8 - 1;
        if outcome != params.token_type as u8 {
            selling_price = 0;
        }

        /* Debit the USDC from user account to probo account */
        let purchase_price = params.token_price * params.quantity;
        let selling_price = selling_price * params.quantity;

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
            "Purchase price: {:?}, Settlement Price: {:?}, commission: {:?}",
            purchase_price,
            selling_price,
            commission,
        );

        let bump = ctx.bumps.delegate.to_be_bytes();
        let seeds = &[b"money", bump.as_ref()];
        let signer_seeds = [&seeds[..]];

        if selling_price > 0 {
            let cpi_accounts = token::Transfer {
                to: ctx.accounts.user_usdc_token_account.to_account_info(),
                from: ctx.accounts.arka_usdc_token_account.to_account_info(),
                authority: ctx.accounts.delegate.to_account_info(),
            };

            let cpi_context = CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                cpi_accounts,
                &signer_seeds,
            );

            token::transfer(cpi_context, amount_to_return)?;
        }

        /* Burn probo token from user account */
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

    pub fn mint_tokens(ctx: Context<MintTokens>, params: MintTokenParams) -> Result<()> {
        // Validate that the price is between (0-1 dollar)
        if params.token_price > ONE_DOLLAR {
            return Err(BuyTokenError::InvalidPrice.into());
        }

        let bump = ctx.bumps.delegate.to_be_bytes();
        let seeds = &[b"money", bump.as_ref()];
        let signer_seeds = [&seeds[..]];

        /* Debit the USDC from user account to probo account */
        let usdc_amount = params.token_price * params.quantity;
        msg!("Total amount of USDC to deduct: {:?}", usdc_amount);

        let cpi_accounts = token::Transfer {
            from: ctx.accounts.user_usdc_token_account.to_account_info(),
            to: ctx.accounts.probo_usdc_token_account.to_account_info(),
            authority: ctx.accounts.delegate.to_account_info(),
        };

        let cpi_context = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            cpi_accounts,
            &signer_seeds,
        );

        token::transfer(cpi_context, usdc_amount)?;

        /* Mint probo token into user account */
        let event_id = params.event_id.to_le_bytes();
        let token_price = params.token_price.to_le_bytes();
        let seeds = &[
            b"eid_",
            event_id.as_ref(),
            b"_tt_",
            &[params.token_type.clone() as u8],
            b"_tp_",
            token_price.as_ref(),
            &[ctx.bumps.probo_mint],
        ];
        let signer = [&seeds[..]];

        mint_to(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                MintTo {
                    authority: ctx.accounts.probo_mint.to_account_info(),
                    to: ctx.accounts.user_probo_token_account.to_account_info(),
                    mint: ctx.accounts.probo_mint.to_account_info(),
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

        msg!(
            "Event created on chain with event_id={:?} and commission_rate={:?}",
            data.event_id,
            data.commission_rate
        );

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
        if params.token_price > ONE_DOLLAR {
            return Err(SellTokenError::InvalidTokenPrice.into());
        }
        if params.selling_price > ONE_DOLLAR {
            return Err(SellTokenError::InvalidSellingPrice.into());
        }

        /* Debit the USDC from user account to probo account */
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
        let seeds = &[b"money", bump.as_ref()];
        let signer_seeds = [&seeds[..]];

        // let program_pda = Pubkey::from_str("EMSsxw9k6kFPp2F4XMdp87q9NwDvAbkMYdRo9BzV1VbP");
        let cpi_accounts = token::Transfer {
            to: ctx.accounts.user_usdc_token_account.to_account_info(),
            from: ctx.accounts.probo_usdc_token_account.to_account_info(),
            authority: ctx.accounts.delegate.to_account_info(),
        };

        let cpi_context = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            cpi_accounts,
            &signer_seeds,
        );

        token::transfer(cpi_context, amount_to_return)?;

        /* Burn probo token from user account */
        let cpi_accounts = token::Burn {
            mint: ctx.accounts.probo_mint.to_account_info(),
            from: ctx.accounts.user_probo_token_account.to_account_info(),
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
}

#[repr(u8)]
#[derive(Debug, PartialEq, Eq, Clone, AnchorSerialize, AnchorDeserialize)]
pub enum TokenType {
    Yes = 0,
    No,
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
    pub event_data: Account<'info, EventData>,
    #[account(mut)]
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
}

#[repr(u8)]
#[derive(Default, Debug, PartialEq, Eq, Clone, AnchorSerialize, AnchorDeserialize)]
pub enum EventOutcome {
    #[default]
    Null = 0,
    Yes,
    No,
}

#[account]
pub struct EventData {
    pub event_id: u64,
    pub comission_rate: u64,
    pub outcome: EventOutcome,
    pub is_outcome_set: bool,
}

impl EventData {
    pub const MAX_PUBKEYS: u64 = 100;
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
    #[account(mut)]
    pub payer: Signer<'info>,
    pub system_program: Program<'info, System>,
}

// Token initialization params
#[derive(AnchorSerialize, AnchorDeserialize, Debug, Clone)]
pub struct MintTokenParams {
    pub token_type: TokenType,
    pub token_price: u64,
    pub event_id: u64,
    pub quantity: u64,
    pub user_id: u64,
    pub user_pubkey: Pubkey,
}

#[derive(Accounts)]
#[instruction(params: MintTokenParams)]
pub struct MintTokens<'info> {
    #[account(
        mut,
        seeds = [b"eid_", params.event_id.to_le_bytes().as_ref(), b"_tt_", &[params.token_type.clone() as u8], b"_tp_", params.token_price.to_le_bytes().as_ref()],
        bump,
        mint::authority = probo_mint,
    )]
    pub probo_mint: Account<'info, Mint>,
    #[account(
        init_if_needed,
        seeds = [b"uid_", params.user_id.to_le_bytes().as_ref(), b"_eid_", params.event_id.to_le_bytes().as_ref(), b"_tt_", &[params.token_type.clone() as u8], b"_tp_", params.token_price.to_le_bytes().as_ref()],
        bump,
        payer = payer,
        token::mint = probo_mint,
        token::authority = payer,
    )]
    pub user_probo_token_account: Account<'info, TokenAccount>,
    #[account(mut)]
    pub user_usdc_token_account: Account<'info, TokenAccount>,
    #[account(mut)]
    pub probo_usdc_token_account: Account<'info, TokenAccount>,
    #[account(mut)]
    pub payer: Signer<'info>,
    pub rent: Sysvar<'info, Rent>,
    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    #[account(signer)]
    pub authority: Signer<'info>,
    /// CHECK: This account is safe as it is used to set the delegate authority for the token account
    #[account(seeds = [b"money"], bump)]
    pub delegate: AccountInfo<'info>,
}

// Token initialization params
#[derive(AnchorSerialize, AnchorDeserialize, Debug, Clone)]
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
        mint::authority = probo_mint,
    )]
    pub probo_mint: Account<'info, Mint>,
    #[account(
        init_if_needed,
        seeds = [b"uid_", params.user_id.to_le_bytes().as_ref(), b"_eid_", params.event_id.to_le_bytes().as_ref(), b"_tt_", &[params.token_type.clone() as u8], b"_tp_", params.token_price.to_le_bytes().as_ref()],
        bump,
        payer = payer,
        token::mint = probo_mint,
        token::authority = payer,
    )]
    pub user_probo_token_account: Account<'info, TokenAccount>,
    #[account(mut)]
    pub user_usdc_token_account: Account<'info, TokenAccount>,
    #[account(mut)]
    pub probo_usdc_token_account: Account<'info, TokenAccount>,
    #[account(mut, signer)]
    pub payer: Signer<'info>,
    pub rent: Sysvar<'info, Rent>,
    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    /// CHECK: This account is safe as it is used to set the delegate authority for the token account
    #[account(seeds = [b"money"], bump)]
    pub delegate: AccountInfo<'info>,
    #[account(signer)]
    pub authority: Signer<'info>,
    pub event_data: Account<'info, EventData>,
}

#[error_code]
pub enum SettleBalanceError {
    #[msg("Event has no outcome set yet!")]
    EventNotFinished,
}

#[derive(AnchorSerialize, AnchorDeserialize, Debug, Clone)]
pub struct SettleBalanceParams {
    pub token_type: TokenType,
    pub token_price: u64,
    pub event_id: u64,
    pub quantity: u64,
    pub user_id: u64,
}

#[derive(Accounts)]
#[instruction(params: SettleBalanceParams)]
pub struct SettleBalance<'info> {
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
    #[account(mut, signer)]
    pub payer: Signer<'info>,
    pub rent: Sysvar<'info, Rent>,
    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    /// CHECK: This account is safe as it is used to set the delegate authority for the token account
    #[account(seeds = [b"money"], bump)]
    pub delegate: AccountInfo<'info>,
    #[account(signer)]
    pub authority: Signer<'info>,
    pub event_data: Account<'info, EventData>,
}
