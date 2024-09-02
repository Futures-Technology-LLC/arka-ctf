use anchor_lang::prelude::*;

#[repr(u8)]
#[derive(AnchorSerialize, AnchorDeserialize, Debug)]
pub enum TokenType {
    Yes = 0,
    No,
}

#[derive(AnchorSerialize, AnchorDeserialize, Debug)]
pub struct ProbonCTFToken {
    quantity: u64,
    price: u64,
    question_id: u64,
    token_type: TokenType,
    user: Pubkey,
}

impl ProbonCTFToken {
    pub fn new(
        quantity: u64,
        price: u64,
        question_id: u64,
        token_type: TokenType,
        user: Pubkey,
    ) -> Self {
        Self {
            quantity,
            price,
            question_id,
            token_type,
            user,
        }
    }

    pub fn mint(&self) {}
}
