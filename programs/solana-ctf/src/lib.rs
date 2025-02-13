use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{self, Mint as OldMint, Token as OldToken, TokenAccount as OldTokenAccount},
    token_interface::TokenAccount,
};
use spl_token::instruction::AuthorityType;
use std::slice::Iter;

pub const OWNER: Pubkey = Pubkey::new_from_array([
    148, 244, 35, 255, 110, 248, 40, 221, 236, 11, 199, 213, 242, 243, 97, 161, 22, 80, 148, 47,
    144, 114, 254, 166, 91, 138, 193, 71, 72, 37, 36, 148,
]);

declare_id!("EEvREcEYzAV31rNmw2QGJHQw54Gbc39vov2fbfCFf7PF");

#[program]
pub mod solana_ctf {
    use super::*;

    pub fn buy_order(ctx: Context<BuyOrder>, params: BuyOrderParams) -> Result<()> {
        // Verify if the signer is the owner
        if ctx.accounts.owner.key() != OWNER {
            return Err(error!(BuyOrderError::Unauthorized));
        }

        // Validate that the price is between (0-1 dollar)
        let event_total_price = ctx.accounts.event_data.event_total_price;
        if params.order_price > event_total_price {
            return Err(BuyOrderError::InvalidPrice.into());
        }

        let bump = ctx.bumps.delegate.to_be_bytes();
        let user_id = params.user_id.to_le_bytes();
        let seeds = &[b"usdc_uid_", user_id.as_ref(), bump.as_ref()];
        let signer_seeds = [&seeds[..]];

        /* Debit the USDC from user account to Arka account */
        let usdc_amount = params.order_price * params.quantity;
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
        let order_price = params.order_price;
        let order_type = params.order_type.clone() as usize;
        let user_event_account = &mut ctx.accounts.user_arka_event_account;

        let current_quantity = user_event_account.total_qty;
        let current_price = user_event_account.avg_purchase_price;

        let new_price = ((current_price[order_type] * current_quantity[order_type])
            + (order_price * quantity))
            / (current_quantity[order_type] + quantity);

        user_event_account.avg_purchase_price[order_type] = new_price;
        user_event_account.total_qty[order_type] += quantity;
        user_event_account.comission = params.commission;

        msg!(
            "Previous avg_price={:?} qty={:?}, New avg_price={:?} qty={:?}",
            current_price[order_type],
            current_quantity[order_type],
            new_price,
            user_event_account.total_qty[order_type],
        );

        Ok(())
    }

    pub fn transfer_from_user_wallet_to_pda(
        ctx: Context<TranferFromUserWallet>,
        data: TranferFromUserWalletParams,
    ) -> Result<()> {
        // Verify if the signer is the owner
        if ctx.accounts.owner.key() != OWNER {
            return Err(error!(TranferFromUserWalletError::Unauthorized));
        }

        let mut amount_from_usdc_wallet = data.amount;
        let mut amount_from_promo_wallet = 0_u64;

        if let Some(promo_account) = &ctx.accounts.promo_account {
            let promo_balance = promo_account.amount;

            amount_from_usdc_wallet = if data.amount > promo_balance {
                data.amount - promo_balance
            } else {
                0_u64
            };

            amount_from_promo_wallet = if data.amount < promo_balance {
                data.amount
            } else {
                promo_balance
            };
        } else {
            msg!("Promo account was not passed to this contract!");
        }

        if amount_from_usdc_wallet > 0 {
            if ctx.accounts.user_usdc_token_account.is_none() {
                return Err(error!(TranferFromUserWalletError::InsufficientBalance));
            }
        }

        if amount_from_promo_wallet > 0 {
            let promo_bump = ctx
                .bumps
                .promo_delegate
                .expect("Failed to get promo delegate")
                .to_be_bytes();
            let user_id_bytes = data.user_id.to_le_bytes();
            let promo_seeds = &[
                b"promo_usdc_uid_",
                user_id_bytes.as_ref(),
                promo_bump.as_ref(),
            ];
            let signer_seeds = [&promo_seeds[..]];

            let cpi_accounts = token::Transfer {
                from: ctx
                    .accounts
                    .promo_account
                    .as_ref()
                    .unwrap()
                    .to_account_info(),
                to: ctx.accounts.escrow_account.to_account_info(),
                authority: ctx
                    .accounts
                    .promo_delegate
                    .clone()
                    .expect("Failed to get promo delegate")
                    .to_account_info(),
            };

            let cpi_context = CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                cpi_accounts,
                &signer_seeds,
            );

            token::transfer(cpi_context, amount_from_promo_wallet)?;
        }

        if amount_from_usdc_wallet > 0 {
            let bump = ctx.bumps.delegate.to_be_bytes();
            let seeds = &[b"money", bump.as_ref()];
            let signer_seeds = [&seeds[..]];

            let cpi_accounts = token::Transfer {
                from: ctx
                    .accounts
                    .user_usdc_token_account
                    .as_ref()
                    .unwrap()
                    .to_account_info(),
                to: ctx.accounts.escrow_account.to_account_info(),
                authority: ctx.accounts.delegate.to_account_info(),
            };

            let cpi_context = CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                cpi_accounts,
                &signer_seeds,
            );

            token::transfer(cpi_context, amount_from_usdc_wallet)?;
        }

        msg!(
            "Locking funds from user wallet to user pda for user_id={:?}, amount={:?}, event_id={:?}, order_id={:?}, promo_amount={:?}",
            data.user_id,
            amount_from_usdc_wallet,
            data.event_id,
            data.order_id,
            amount_from_promo_wallet
        );

        Ok(())
    }

    pub fn transfer_from_user_pda_to_wallet(
        ctx: Context<TranferFromUserPda>,
        data: TranferFromUserPdaParams,
    ) -> Result<()> {
        // Verify if the signer is the owner
        if ctx.accounts.owner.key() != OWNER {
            return Err(error!(TranferFromUserPdaError::Unauthorized));
        }

        msg!(
            "Releasing funds from user pda to user wallet for user_id={:?}, amount={:?} order_id={:?}, event_id={:?}, utr_id={:?}",
            data.user_id,
            data.amount,
            data.order_id,
            data.event_id,
            data.utr_id
        );

        let bump = ctx.bumps.delegate.to_be_bytes();
        let user_id = data.user_id.to_le_bytes();
        let seeds = &[b"usdc_uid_", user_id.as_ref(), bump.as_ref()];
        let signer_seeds = [&seeds[..]];

        let usdc_account_amount = data.amount - data.promo_amount;

        if let Some(usdc_account) = &ctx.accounts.user_usdc_token_account {
            if usdc_account_amount > 0 {
                let cpi_accounts = token::Transfer {
                    to: usdc_account.to_account_info(),
                    from: ctx.accounts.escrow_account.to_account_info(),
                    authority: ctx.accounts.delegate.to_account_info(),
                };

                let cpi_context = CpiContext::new_with_signer(
                    ctx.accounts.token_program.to_account_info(),
                    cpi_accounts,
                    &signer_seeds,
                );

                token::transfer(cpi_context, usdc_account_amount)?;
            }
        } else {
            msg!("Usdc account does not exists!");
        }

        if let Some(promo_account) = &ctx.accounts.promo_account {
            if data.promo_amount > 0 {
                let cpi_accounts = token::Transfer {
                    to: promo_account.to_account_info(),
                    from: ctx.accounts.escrow_account.to_account_info(),
                    authority: ctx.accounts.delegate.to_account_info(),
                };

                let cpi_context = CpiContext::new_with_signer(
                    ctx.accounts.token_program.to_account_info(),
                    cpi_accounts,
                    &signer_seeds,
                );

                token::transfer(cpi_context, data.promo_amount)?;
            }
        } else {
            msg!("Promo account was not passed to this contract!");
        }

        Ok(())
    }

    pub fn initialize_user_ata(
        ctx: Context<InitializeUserAta>,
        data: InitUserAtaParams,
    ) -> Result<()> {
        // Verify if the signer is the owner
        if ctx.accounts.owner.key() != OWNER {
            return Err(error!(InitializeUserAtaError::Unauthorized));
        }

        msg!("User ata created for user_id={:?}", data.user_id,);

        /* Create escrow account for storing order-init balance */
        let bump = ctx.bumps.escrow_account.to_be_bytes();
        let user_id_bytes = data.user_id.to_le_bytes();
        let seeds = &[b"usdc_uid_", user_id_bytes.as_ref(), bump.as_ref()];
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

        /* Create promo account to store promo balance */
        let promo_bump = ctx.bumps.promo_account.to_be_bytes();
        let promo_seeds = &[
            b"promo_usdc_uid_",
            user_id_bytes.as_ref(),
            promo_bump.as_ref(),
        ];
        let promo_signer_seeds = [&promo_seeds[..]];

        token::set_authority(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                token::SetAuthority {
                    account_or_mint: ctx.accounts.promo_account.to_account_info(),
                    current_authority: ctx.accounts.promo_delegate.to_account_info(),
                },
                &promo_signer_seeds,
            ),
            AuthorityType::AccountOwner,
            Some(ctx.accounts.promo_delegate.key()), // Set the PDA as the new owner
        )?;

        let bump = ctx.bumps.arka_delegate.to_be_bytes();
        let seeds = &[b"money", bump.as_ref()];
        let signer_seeds = [&seeds[..]];

        let cpi_accounts = token::Transfer {
            from: ctx.accounts.arka_usdc_wallet.to_account_info(),
            to: ctx.accounts.promo_account.to_account_info(),
            authority: ctx.accounts.arka_delegate.to_account_info(),
        };

        let cpi_context = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            cpi_accounts,
            &signer_seeds,
        );

        token::transfer(cpi_context, data.promo_balance)?;

        Ok(())
    }

    pub fn initialize_event(ctx: Context<InitializeEvent>, data: InitEventParams) -> Result<()> {
        // Verify if the signer is the owner
        if ctx.accounts.owner.key() != OWNER {
            return Err(error!(InitializeEventError::Unauthorized));
        }

        ctx.accounts.event_data.event_id = data.event_id;
        ctx.accounts.event_data.outcome = EventOutcome::Null;
        ctx.accounts.event_data.is_outcome_set = false;
        ctx.accounts.event_data.event_total_price = data.event_total_price;

        msg!(
            "Event created on chain with event_id={:?}, event_price={:?}",
            data.event_id,
            data.event_total_price,
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
        // Verify if the signer is the owner
        if ctx.accounts.owner.key() != OWNER {
            return Err(error!(UpdateOutcomeError::Unauthorized));
        }

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
        ctx: Context<CloseEventAccount>,
        params: CloseEventAccountParams,
    ) -> Result<()> {
        // Verify if the signer is the owner
        if ctx.accounts.owner.key() != OWNER {
            return Err(error!(CloseEventAccountError::Unauthorized));
        }

        msg!("Closing event account with event_id={:?}", params.event_id);
        Ok(())
    }

    pub fn close_user_event_data(
        ctx: Context<CloseUserEventAccount>,
        params: CloseUserEventAccountParams,
    ) -> Result<()> {
        // Verify if the signer is the owner
        if ctx.accounts.owner.key() != OWNER {
            return Err(error!(CloseUserEventError::Unauthorized));
        }

        let user_account = &ctx.accounts.user_event_data;
        for order_type in solana_ctf::OrderType::iterator() {
            let qty = user_account.total_qty[order_type.clone() as usize];
            if qty > 0 {
                msg!("Pending qty={:?} order_type={:?}", qty, order_type);
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

    pub fn initialize_promo_account(
        ctx: Context<InitializePromoAccount>,
        data: InitPromoAccount,
    ) -> Result<()> {
        // Verify if the signer is the owner
        if ctx.accounts.owner.key() != OWNER {
            return Err(error!(InitializeUserAtaError::Unauthorized));
        }

        msg!("User promo ata created for user_id={:?}", data.user_id,);

        let user_id_bytes = data.user_id.to_le_bytes();
        /* Create promo account to store promo balance */
        let promo_bump = ctx.bumps.promo_account.to_be_bytes();
        let promo_seeds = &[
            b"promo_usdc_uid_",
            user_id_bytes.as_ref(),
            promo_bump.as_ref(),
        ];
        let promo_signer_seeds = [&promo_seeds[..]];

        token::set_authority(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                token::SetAuthority {
                    account_or_mint: ctx.accounts.promo_account.to_account_info(),
                    current_authority: ctx.accounts.promo_delegate.to_account_info(),
                },
                &promo_signer_seeds,
            ),
            AuthorityType::AccountOwner,
            Some(ctx.accounts.promo_delegate.key()), // Set the PDA as the new owner
        )?;

        Ok(())
    }

    pub fn sell_order(ctx: Context<SellOrder>, params: SellOrderParams) -> Result<()> {
        // Verify if the signer is the owner
        if ctx.accounts.owner.key() != OWNER {
            return Err(error!(SellOrderError::Unauthorized));
        }

        // Validate that the price is between (0-1 dollar)
        let order_type = params.order_type as usize;
        let event_total_price = ctx.accounts.event_data.event_total_price;
        if params.order_price > event_total_price {
            return Err(SellOrderError::InvalidTokenPrice.into());
        }

        if params.selling_price == event_total_price {
            if !ctx.accounts.event_data.is_outcome_set {
                return Err(SellOrderError::EventNotFinished.into());
            }

            let outcome = ctx.accounts.event_data.outcome.clone() as u8 - 1;
            if outcome != params.order_type as u8 {
                return Err(SellOrderError::EventOutcomeMismatch.into());
            }
        }

        /* Credit the USDC from user account to Arka account */
        let avg_purchase_price =
            ctx.accounts.user_arka_event_account.avg_purchase_price[order_type];
        let total_qty = ctx.accounts.user_arka_event_account.total_qty[order_type];

        if ctx.accounts.event_data.is_outcome_set
            && ctx.accounts.event_data.outcome == EventOutcome::Void
        {
            if params.selling_price != avg_purchase_price {
                return Err(SellOrderError::EventOutcomeMismatch.into());
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
            "Purchase price: {:?}, Selling Price: {:?}, commission: {:?} promo_amount: {:?}",
            purchase_price,
            selling_price,
            commission,
            params.promo_amount,
        );

        let bump = ctx.bumps.delegate.to_be_bytes();
        let event_id = params.event_id.to_le_bytes();
        let seeds = &[b"usdc_eid_", event_id.as_ref(), bump.as_ref()];
        let signer_seeds = [&seeds[..]];

        if params.selling_price > 0 {
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

        if let Some(usdc_account) = &ctx.accounts.user_usdc_token_account {
            if amount_to_return - params.promo_amount > 0 {
                let cpi_accounts = token::Transfer {
                    to: usdc_account.to_account_info(),
                    from: ctx.accounts.arka_usdc_event_token_account.to_account_info(),
                    authority: ctx.accounts.delegate.to_account_info(),
                };

                let cpi_context = CpiContext::new_with_signer(
                    ctx.accounts.old_token_program.to_account_info(),
                    cpi_accounts,
                    &signer_seeds,
                );

                token::transfer(cpi_context, amount_to_return - params.promo_amount)?;
            }
        }

        if let Some(promo_account) = &ctx.accounts.promo_account {
            if params.promo_amount > 0 {
                let cpi_accounts = token::Transfer {
                    to: promo_account.to_account_info(),
                    from: ctx.accounts.arka_usdc_event_token_account.to_account_info(),
                    authority: ctx.accounts.delegate.to_account_info(),
                };

                let cpi_context = CpiContext::new_with_signer(
                    ctx.accounts.old_token_program.to_account_info(),
                    cpi_accounts,
                    &signer_seeds,
                );

                token::transfer(cpi_context, params.promo_amount)?;
            }
        } else {
            msg!("Promo account was not passed to this contract!");
        }

        /* Reduce Arka token quantity from user account */
        ctx.accounts.user_arka_event_account.total_qty[order_type] -= params.quantity;
        msg!(
            "Total quantiy available after this trade={:?}",
            ctx.accounts.user_arka_event_account.total_qty[order_type]
        );

        Ok(())
    }
}

#[error_code]
pub enum BuyOrderError {
    #[msg("Price > 100")]
    InvalidPrice,
    #[msg("Unauthorized: Only the owner can execute this instruction.")]
    Unauthorized,
}

#[error_code]
pub enum TranferFromUserWalletError {
    #[msg("Unauthorized: Only the owner can execute this instruction.")]
    Unauthorized,
    #[msg("Insufficient balance")]
    InsufficientBalance,
}

#[error_code]
pub enum TranferFromUserPdaError {
    #[msg("Unauthorized: Only the owner can execute this instruction.")]
    Unauthorized,
}

#[error_code]
pub enum InitializeUserAtaError {
    #[msg("Unauthorized: Only the owner can execute this instruction.")]
    Unauthorized,
}

#[error_code]
pub enum CloseEventDataError {
    #[msg("Unauthorized: Only the owner can execute this instruction.")]
    Unauthorized,
}

#[error_code]
pub enum CloseEventAccountError {
    #[msg("Unauthorized: Only the owner can execute this instruction.")]
    Unauthorized,
}

#[error_code]
pub enum SellOrderError {
    #[msg("Price > 100")]
    InvalidTokenPrice,
    #[msg("Price > 100")]
    InvalidSellingPrice,
    #[msg("Event has no outcome set yet!")]
    EventNotFinished,
    #[msg("Event outcome does not match price!")]
    EventOutcomeMismatch,
    #[msg("Unauthorized: Only the owner can execute this instruction.")]
    Unauthorized,
}

#[repr(u8)]
#[derive(
    Debug, PartialEq, Eq, Clone, AnchorSerialize, AnchorDeserialize, serde::Deserialize, Copy,
)]
pub enum OrderType {
    Yes = 0,
    No,
}

impl OrderType {
    pub fn iterator() -> Iter<'static, OrderType> {
        static ORDER_TYPES: [OrderType; 2] = [OrderType::Yes, OrderType::No];
        ORDER_TYPES.iter()
    }
}

#[derive(Accounts)]
pub struct UpdateOutcome<'info> {
    /// CHECK: This account is safe since this is our owner account.
    #[account(signer)]
    pub owner: Signer<'info>,
    #[account(mut)]
    pub event_data: Account<'info, EventData>,
    #[account(mut)]
    pub payer: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[error_code]
pub enum UpdateOutcomeError {
    #[msg("Trying to set outcome to Null, not allowed.")]
    InvalidOutcomeState,
    #[msg("You are only allowed to update once.")]
    OutcomeAlreadyUpdated,
    #[msg("Unauthorized: Only the owner can execute this instruction.")]
    Unauthorized,
}

#[derive(AnchorSerialize, AnchorDeserialize, Debug, Clone)]
pub struct InitEventParams {
    pub event_id: u64,
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
            3 => EventOutcome::Void,
            _ => EventOutcome::Null, // Default case if value doesn't match
        }
    }
}

#[account]
pub struct EventData {
    pub event_id: u64,
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
    #[msg("Unauthorized: Only the owner can execute this instruction.")]
    Unauthorized,
}

#[derive(AnchorSerialize, AnchorDeserialize, Debug, Clone)]
pub struct InitUserAtaParams {
    pub user_id: u64,
    pub promo_balance: u64,
}

#[derive(AnchorSerialize, AnchorDeserialize, Debug, Clone)]
pub struct InitPromoAccount {
    pub user_id: u64,
}

#[derive(Accounts)]
#[instruction(params: InitUserAtaParams)]
pub struct InitializePromoAccount<'info> {
    /// CHECK: This account is safe since this is our owner account.
    #[account(signer)]
    pub owner: Signer<'info>,
    pub usdc_mint: Box<Account<'info, OldMint>>,
    #[account(
        init,
        seeds = [b"promo_usdc_uid_", params.user_id.to_le_bytes().as_ref()],
        bump,
        payer = payer,
        token::mint = usdc_mint,
        token::authority = promo_delegate,
    )]
    pub promo_account: Box<Account<'info, OldTokenAccount>>,
    #[account(
        seeds = [b"promo_usdc_uid_", params.user_id.to_le_bytes().as_ref()],
        bump,
    )]
    /// CHECK: This account is safe as it is used to set the delegate authority for the token account
    pub promo_delegate: AccountInfo<'info>,
    #[account(mut)]
    pub payer: Signer<'info>,
    pub system_program: Program<'info, System>,
    pub rent: Sysvar<'info, Rent>,
    pub token_program: Program<'info, OldToken>,
    pub associated_token_program: Program<'info, AssociatedToken>,
}

#[derive(Accounts)]
#[instruction(params: InitUserAtaParams)]
pub struct InitializeUserAta<'info> {
    /// CHECK: This account is safe since this is our owner account.
    #[account(signer)]
    pub owner: Signer<'info>,
    pub usdc_mint: Box<Account<'info, OldMint>>,
    #[account(
        init,
        seeds = [b"usdc_uid_", params.user_id.to_le_bytes().as_ref()],
        bump,
        payer = payer,
        token::mint = usdc_mint,
        token::authority = delegate,
    )]
    pub escrow_account: Box<Account<'info, OldTokenAccount>>,
    #[account(
        init,
        seeds = [b"promo_usdc_uid_", params.user_id.to_le_bytes().as_ref()],
        bump,
        payer = payer,
        token::mint = usdc_mint,
        token::authority = promo_delegate,
    )]
    pub promo_account: Box<Account<'info, OldTokenAccount>>,
    #[account(
        seeds = [b"usdc_uid_", params.user_id.to_le_bytes().as_ref()],
        bump,
    )]
    /// CHECK: This account is safe as it is used to set the delegate authority for the token account
    pub delegate: AccountInfo<'info>,
    #[account(
        seeds = [b"promo_usdc_uid_", params.user_id.to_le_bytes().as_ref()],
        bump,
    )]
    /// CHECK: This account is safe as it is used to set the delegate authority for the token account
    pub promo_delegate: AccountInfo<'info>,
    #[account(mut)]
    pub arka_usdc_wallet: Box<Account<'info, OldTokenAccount>>,
    /// CHECK: This account is safe as it is used to set the delegate authority for the token account
    #[account(seeds = [b"money"], bump)]
    pub arka_delegate: AccountInfo<'info>,
    #[account(mut)]
    pub payer: Signer<'info>,
    pub system_program: Program<'info, System>,
    pub rent: Sysvar<'info, Rent>,
    pub token_program: Program<'info, OldToken>,
    pub associated_token_program: Program<'info, AssociatedToken>,
}

#[derive(AnchorSerialize, AnchorDeserialize, Debug, Clone)]
pub struct TranferFromUserWalletParams {
    pub user_id: u64,
    pub amount: u64,
    pub event_id: u64,
    pub order_id: u64,
    pub promo_amount: u64,
}

#[derive(Accounts)]
#[instruction(params: TranferFromUserWalletParams)]
pub struct TranferFromUserWallet<'info> {
    /// CHECK: This account is safe since this is our owner account.
    #[account(signer)]
    pub owner: Signer<'info>,
    pub usdc_mint: Account<'info, OldMint>,
    #[account(mut)]
    pub user_usdc_token_account: Option<Box<InterfaceAccount<'info, TokenAccount>>>,
    #[account(
        mut,
        seeds = [b"usdc_uid_", params.user_id.to_le_bytes().as_ref()],
        bump,
    )]
    pub escrow_account: Box<Account<'info, OldTokenAccount>>,
    #[account(
        mut,
        seeds = [b"promo_usdc_uid_", params.user_id.to_le_bytes().as_ref()],
        bump,
    )]
    pub promo_account: Option<Box<Account<'info, OldTokenAccount>>>,
    /// CHECK: This account is safe as it is used to set the delegate authority for the token account
    #[account(seeds = [b"money"], bump)]
    pub delegate: AccountInfo<'info>,
    /// CHECK: This account is safe as it is used to set the delegate authority for the promo account
    #[account(seeds = [b"promo_usdc_uid_", params.user_id.to_le_bytes().as_ref()], bump)]
    pub promo_delegate: Option<AccountInfo<'info>>,
    #[account(mut)]
    pub payer: Signer<'info>,
    pub system_program: Program<'info, System>,
    pub rent: Sysvar<'info, Rent>,
    pub token_program: Program<'info, OldToken>,
    pub associated_token_program: Program<'info, AssociatedToken>,
}

#[derive(AnchorSerialize, AnchorDeserialize, Debug, Clone)]
pub struct TranferFromUserPdaParams {
    pub user_id: u64,
    pub amount: u64,
    pub event_id: u64,
    pub order_id: u64,
    pub utr_id: String,
    pub promo_amount: u64,
}

#[derive(Accounts)]
#[instruction(params: TranferFromUserPdaParams)]
pub struct TranferFromUserPda<'info> {
    /// CHECK: This account is safe since this is our owner account.
    #[account(signer)]
    pub owner: Signer<'info>,
    pub usdc_mint: Account<'info, OldMint>,
    #[account(mut)]
    pub user_usdc_token_account: Option<Box<InterfaceAccount<'info, TokenAccount>>>,
    #[account(
        mut,
        seeds = [b"usdc_uid_", params.user_id.to_le_bytes().as_ref()],
        bump,
    )]
    pub escrow_account: Account<'info, OldTokenAccount>,
    /// CHECK: This account is safe as it is used to set the delegate authority for the token account
    #[account(
        seeds = [b"usdc_uid_", params.user_id.to_le_bytes().as_ref()],
        bump,
    )]
    pub delegate: AccountInfo<'info>,
    #[account(
        mut,
        seeds = [b"promo_usdc_uid_", params.user_id.to_le_bytes().as_ref()],
        bump,
    )]
    pub promo_account: Option<Box<Account<'info, OldTokenAccount>>>,
    #[account(mut)]
    pub payer: Signer<'info>,
    pub system_program: Program<'info, System>,
    pub rent: Sysvar<'info, Rent>,
    pub token_program: Program<'info, OldToken>,
    pub associated_token_program: Program<'info, AssociatedToken>,
}

#[derive(Accounts)]
#[instruction(params: InitEventParams)]
pub struct InitializeEvent<'info> {
    /// CHECK: This account is safe since this is our owner account.
    #[account(signer)]
    pub owner: Signer<'info>,
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
pub struct BuyOrderParams {
    pub order_type: OrderType,
    pub order_price: u64,
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
#[instruction(params: BuyOrderParams)]
pub struct BuyOrder<'info> {
    /// CHECK: This account is safe since this is our owner account.
    #[account(signer)]
    pub owner: Signer<'info>,
    #[account(
        init_if_needed,
        seeds = [b"uid_", params.user_id.to_le_bytes().as_ref(), b"_eid_", params.event_id.to_le_bytes().as_ref()],
        bump,
        payer = payer,
        space = 8 + UserEventData::LEN,
    )]
    pub user_arka_event_account: Account<'info, UserEventData>,
    #[account(
        mut,
        seeds = [b"usdc_uid_", params.user_id.to_le_bytes().as_ref()],
        bump,
    )]
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
    /// CHECK: This account is safe as it is used to set the delegate authority for the token account
    #[account(
        seeds = [b"usdc_uid_", params.user_id.to_le_bytes().as_ref()],
        bump,
    )]
    pub delegate: AccountInfo<'info>,
    pub event_data: Account<'info, EventData>,
}

// Token initialization params
#[derive(AnchorSerialize, AnchorDeserialize, Debug, Clone, serde::Deserialize)]
pub struct SellOrderParams {
    pub order_type: OrderType,
    pub order_price: u64,
    pub event_id: u64,
    pub quantity: u64,
    pub user_id: u64,
    pub selling_price: u64,
    pub promo_amount: u64,
}

#[derive(Accounts)]
#[instruction(params: SellOrderParams)]
pub struct SellOrder<'info> {
    /// CHECK: This account is safe since this is our owner account.
    #[account(signer)]
    pub owner: Signer<'info>,
    #[account(
        mut,
        seeds = [b"uid_", params.user_id.to_le_bytes().as_ref(), b"_eid_", params.event_id.to_le_bytes().as_ref()],
        bump,
    )]
    pub user_arka_event_account: Account<'info, UserEventData>,
    #[account(mut)]
    pub user_usdc_token_account: Option<Box<InterfaceAccount<'info, TokenAccount>>>,
    #[account(
        mut,
        seeds = [b"usdc_eid_", params.event_id.to_le_bytes().as_ref()],
        bump,
    )]
    pub arka_usdc_event_token_account: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(
        mut,
        seeds = [b"promo_usdc_uid_", params.user_id.to_le_bytes().as_ref()],
        bump,
    )]
    pub promo_account: Option<Box<InterfaceAccount<'info, TokenAccount>>>,
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
    pub event_data: Account<'info, EventData>,
}

#[derive(AnchorSerialize, AnchorDeserialize, Debug, Clone, serde::Deserialize)]
pub struct CloseEventAccountParams {
    pub event_id: u64,
}

#[derive(Accounts)]
#[instruction(params: CloseEventAccountParams)]
pub struct CloseEventAccount<'info> {
    /// CHECK: This account is safe since this is our owner account.
    #[account(signer)]
    pub owner: Signer<'info>,
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
    #[msg("Unauthorized: Only the owner can execute this instruction.")]
    Unauthorized,
}

#[derive(AnchorSerialize, AnchorDeserialize, Debug, Clone, serde::Deserialize)]
pub struct CloseUserEventAccountParams {
    pub event_id: u64,
    pub user_id: u64,
}

#[derive(Accounts)]
#[instruction(params: CloseUserEventAccountParams)]
pub struct CloseUserEventAccount<'info> {
    /// CHECK: This account is safe since this is our owner account.
    #[account(signer)]
    pub owner: Signer<'info>,
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
