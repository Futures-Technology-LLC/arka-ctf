use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{self, Mint as OldMint, Token as OldToken, TokenAccount as OldTokenAccount},
    token_interface::TokenAccount,
};
use spl_token::instruction::AuthorityType;
use std::slice::Iter;

declare_id!("EcVyZwmzssLbWBnnf7gSqVkZmc96iTNaYe4jQTrfHDLA");

#[program]
pub mod solana_ctf {
    use super::*;

    pub fn buy_tokens(ctx: Context<BuyTokens>, params: BuyTokenParams) -> Result<()> {
        // Validate that the price is between (0-1 dollar)
        let event_total_price = ctx.accounts.event_data.event_total_price;
        if params.token_price > event_total_price {
            return Err(BuyTokenError::InvalidPrice.into());
        }

        let bump = ctx.bumps.delegate.to_be_bytes();
        let seeds = &[b"money", bump.as_ref()];
        let signer_seeds = [&seeds[..]];

        /* Debit the USDC from user account to Arka account */
        let usdc_amount = params.token_price * params.quantity;
        msg!("Total amount of USDC to deduct: {:?}", usdc_amount);

        let cpi_accounts = token::Transfer {
            from: ctx.accounts.user_usdc_token_account.to_account_info(),
            to: ctx.accounts.arka_usdc_event_token_account.to_account_info(),
            authority: ctx.accounts.delegate.to_account_info(),
        };

        let cpi_context = CpiContext::new_with_signer(
            ctx.accounts.old_token_program.to_account_info(),
            cpi_accounts,
            &signer_seeds,
        );

        token::transfer(cpi_context, usdc_amount)?;

        /* Mint Arka token into user account */
        let quantity = params.quantity;
        let token_price = params.token_price;
        let token_type = params.token_type.clone() as usize;
        let user_event_account = &mut ctx.accounts.user_arka_event_account;

        let current_quantity = user_event_account.total_qty;
        let current_price = user_event_account.avg_purchase_price;

        let new_price = ((current_price[token_type] * current_quantity[token_type])
            + (token_price * quantity))
            / (current_quantity[token_type] + quantity);

        user_event_account.avg_purchase_price[token_type] = new_price;
        user_event_account.total_qty[token_type] += quantity;
        user_event_account.comission = params.commission;

        msg!(
            "Previous avg_price={:?} qty={:?}, New avg_price={:?} qty={:?}",
            current_price[token_type],
            current_quantity[token_type],
            new_price,
            user_event_account.total_qty[token_type],
        );

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

    pub fn close_event_data(
        _ctx: Context<CloseEventAccount>,
        params: CloseEventAccountParams,
    ) -> Result<()> {
        msg!("Closing event account with event_id={:?}", params.event_id);
        Ok(())
    }

    pub fn close_user_event_data(
        ctx: Context<CloseUserEventAccount>,
        params: CloseUserEventAccountParams,
    ) -> Result<()> {
        let user_account = &ctx.accounts.user_event_data;
        for token_type in solana_ctf::TokenType::iterator() {
            let qty = user_account.total_qty[token_type.clone() as usize];
            if qty > 0 {
                msg!("Pending qty={:?} token_type={:?}", qty, token_type);
                return Err(CloseUserEventError::PendingQuantity.into());
            }
        }
        msg!(
            "Successfully close user_account_id={:?} event_id={:?}",
            params.user_id,
            params.event_id
        );
        Ok(())
    }

    pub fn sell_tokens(ctx: Context<SellTokens>, params: SellTokenParams) -> Result<()> {
        // Validate that the price is between (0-1 dollar)
        let token_type = params.token_type as usize;
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

        /* Credit the USDC from user account to Arka account */
        let avg_purchase_price =
            ctx.accounts.user_arka_event_account.avg_purchase_price[token_type];
        let total_qty = ctx.accounts.user_arka_event_account.total_qty[token_type];

        if ctx.accounts.event_data.is_outcome_set
            && ctx.accounts.event_data.outcome == EventOutcome::Void
        {
            if params.selling_price != avg_purchase_price {
                return Err(SellTokenError::EventOutcomeMismatch.into());
            }
        }

        assert!(total_qty >= params.quantity);

        let purchase_price = avg_purchase_price * params.quantity;
        let selling_price = params.selling_price * params.quantity;

        let mut amount_to_return = selling_price;
        let mut commission = 0;

        // User is making a profit, thus we need to deduct commission
        if selling_price > purchase_price {
            let commission_rate = ctx.accounts.user_arka_event_account.comission;
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
                ctx.accounts.old_token_program.to_account_info(),
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
                ctx.accounts.old_token_program.to_account_info(),
                cpi_accounts,
                &signer_seeds,
            );

            token::transfer(cpi_context, commission)?;
        }

        /* Reduce Arka token quantity from user account */
        ctx.accounts.user_arka_event_account.total_qty[token_type] -= params.quantity;
        msg!(
            "Total quantiy available after this trade={:?}",
            ctx.accounts.user_arka_event_account.total_qty[token_type]
        );

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

#[derive(Accounts)]
pub struct UpdateOutcome<'info> {
    #[account(mut)]
    pub event_data: Account<'info, EventData>,
    pub user: Signer<'info>,
    pub system_program: Program<'info, System>,
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
    Void,
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
        bump,
    )]
    pub event_data: Account<'info, EventData>,
    pub usdc_mint: Account<'info, OldMint>,
    #[account(
        init,
        seeds = [b"usdc_eid_", params.event_id.to_le_bytes().as_ref()],
        bump,
        payer = payer,
        token::mint = usdc_mint,
        token::authority = delegate,
    )]
    pub escrow_account: Account<'info, OldTokenAccount>,
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
    pub token_program: Program<'info, OldToken>,
    pub associated_token_program: Program<'info, AssociatedToken>,
}

// Token initialization params
#[derive(AnchorSerialize, AnchorDeserialize, Debug, Clone)]
pub struct BuyTokenParams {
    pub token_type: TokenType,
    pub token_price: u64,
    pub event_id: u64,
    pub quantity: u64,
    pub user_id: u64,
    pub commission: u64,
}

#[account]
pub struct UserEventData {
    pub avg_purchase_price: [u64; 2],
    pub total_qty: [u64; 2],
    pub comission: u64,
}

impl UserEventData {
    pub const LEN: usize = std::mem::size_of::<UserEventData>();
}

#[derive(Accounts)]
#[instruction(params: BuyTokenParams)]
pub struct BuyTokens<'info> {
    #[account(
        init_if_needed,
        seeds = [b"uid_", params.user_id.to_le_bytes().as_ref(), b"_eid_", params.event_id.to_le_bytes().as_ref()],
        bump,
        payer = payer,
        space = 8 + UserEventData::LEN,
    )]
    pub user_arka_event_account: Account<'info, UserEventData>,
    #[account(mut)]
    pub user_usdc_token_account: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(
        mut,
        seeds = [b"usdc_eid_", params.event_id.to_le_bytes().as_ref()],
        bump,
    )]
    pub arka_usdc_event_token_account: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(mut)]
    pub payer: Signer<'info>,
    pub rent: Sysvar<'info, Rent>,
    pub system_program: Program<'info, System>,
    pub old_token_program: Program<'info, OldToken>,
    #[account(signer)]
    pub authority: Signer<'info>,
    /// CHECK: This account is safe as it is used to set the delegate authority for the token account
    #[account(seeds = [b"money"], bump)]
    pub delegate: AccountInfo<'info>,
    pub event_data: Account<'info, EventData>,
}

// Token initialization params
#[derive(AnchorSerialize, AnchorDeserialize, Debug, Clone, serde::Deserialize)]
pub struct SellTokenParams {
    pub token_type: TokenType,
    pub token_price: u64,
    pub event_id: u64,
    pub quantity: u64,
    pub user_id: u64,
    pub selling_price: u64,
}

#[derive(Accounts)]
#[instruction(params: SellTokenParams)]
pub struct SellTokens<'info> {
    #[account(
        mut,
        seeds = [b"uid_", params.user_id.to_le_bytes().as_ref(), b"_eid_", params.event_id.to_le_bytes().as_ref()],
        bump,
    )]
    pub user_arka_event_account: Account<'info, UserEventData>,
    #[account(mut)]
    pub user_usdc_token_account: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(
        mut,
        seeds = [b"usdc_eid_", params.event_id.to_le_bytes().as_ref()],
        bump,
    )]
    pub arka_usdc_event_token_account: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(mut)]
    pub arka_usdc_token_account: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(mut, signer)]
    pub payer: Signer<'info>,
    pub rent: Sysvar<'info, Rent>,
    pub system_program: Program<'info, System>,
    pub old_token_program: Program<'info, OldToken>,
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

#[derive(AnchorSerialize, AnchorDeserialize, Debug, Clone, serde::Deserialize)]
pub struct CloseEventAccountParams {
    pub event_id: u64,
}

#[derive(Accounts)]
#[instruction(params: CloseEventAccountParams)]
pub struct CloseEventAccount<'info> {
    #[account(
        mut,
        seeds = [b"eid_", params.event_id.to_le_bytes().as_ref()],
        bump,
        close = payer,
    )]
    pub event_data: Account<'info, EventData>,
    #[account(signer)]
    pub payer: Signer<'info>,
}

#[error_code]
pub enum CloseUserEventError {
    #[msg("User has not liquidated all his position.")]
    PendingQuantity,
}

#[derive(AnchorSerialize, AnchorDeserialize, Debug, Clone, serde::Deserialize)]
pub struct CloseUserEventAccountParams {
    pub event_id: u64,
    pub user_id: u64,
}

#[derive(Accounts)]
#[instruction(params: CloseUserEventAccountParams)]
pub struct CloseUserEventAccount<'info> {
    #[account(
        mut,
        seeds = [b"uid_", params.user_id.to_le_bytes().as_ref(), b"_eid_", params.event_id.to_le_bytes().as_ref()],
        bump,
        close = payer,
    )]
    pub user_event_data: Account<'info, UserEventData>,
    #[account(signer)]
    pub payer: Signer<'info>,
}
