use std::str::FromStr;

use anchor_lang::prelude::*;
use anchor_lang::InstructionData;
use anchor_spl::token::TokenAccount;
use anchor_spl::token::ID as OLD_TOKEN_PROGRAM_ID;
use anchor_spl::token_2022::ID as TOKEN_PROGRAM_ID;
use solana_program_test::*;
use solana_sdk::hash::Hash;
use solana_sdk::program_pack::Pack;
use solana_sdk::{
    instruction::Instruction,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    system_program,
    sysvar::rent::ID as SYSVAR_RENT_PUBKEY,
    transaction::Transaction,
};
use spl_associated_token_account::get_associated_token_address;
use spl_token::instruction::approve;
use spl_token_2022::instruction::initialize_mint;
use spl_token_2022::instruction::mint_to;

pub const ONE_DOLLAR: u64 = 1_000_000;

pub struct UsdcMint {
    pub mint: Keypair,
    pub mint_authority: Keypair,
}

// #[derive(Debug)]
pub struct User {
    pub user_key: Keypair,
    pub user_usdc_ata: Pubkey,
}

async fn create_usdc_mint(
    bank_client: &mut BanksClient,
    payer: &Keypair,
    recent_blockhash: Hash,
) -> UsdcMint {
    let mint = Keypair::new();
    let mint_authority = Keypair::new();
    let mint_decimals = 6;
    let mint_rent = bank_client
        .clone()
        .get_rent()
        .await
        .unwrap()
        .minimum_balance(spl_token::state::Mint::LEN);

    println!("mint_rent_for_usdc_account={:?}", mint_rent);
    let create_mint_account_ix = solana_sdk::system_instruction::create_account(
        &payer.pubkey(),                    // Payer
        &mint.pubkey(),                     // New mint account
        mint_rent,                          // Rent-exempt balance
        spl_token::state::Mint::LEN as u64, // Space for the mint account
        &spl_token::id(),                   // Program ID of the SPL Token program
    );

    // Initialize the mint account (this sets mint authority, freeze authority, decimals)
    let init_mint_ix = initialize_mint(
        &spl_token::id(),         // Program ID of the SPL Token program
        &mint.pubkey(),           // Mint account public key
        &mint_authority.pubkey(), // Mint authority
        None,                     // Freeze authority (None if you don't want one)
        mint_decimals,            // Mint decimals (9 for most SPL tokens)
    )
    .unwrap();

    let mut transaction = Transaction::new_with_payer(
        &[create_mint_account_ix, init_mint_ix],
        Some(&payer.pubkey()), // Payer for the transaction
    );

    transaction.sign(&[&payer, &mint], recent_blockhash);

    bank_client.process_transaction(transaction).await.unwrap();

    UsdcMint {
        mint,
        mint_authority,
    }
}

async fn get_usdc_account(bank_client: &mut BanksClient, user_usdc_ata: &Pubkey) -> TokenAccount {
    // Fetch the associated token account data
    let ata_account = bank_client
        .get_account(user_usdc_ata.clone())
        .await
        .expect("failed to get account")
        .expect("ATA not found");

    // Deserialize the account data into a TokenAccount struct
    let token_account = TokenAccount::try_deserialize(&mut &ata_account.data[..]).unwrap();
    println!("token_account={:?}", token_account);

    // Get the balance (in the smallest unit of the token, like lamports)
    token_account
}

async fn mint_usdc(
    bank_client: &mut BanksClient,
    usdc_mint: &UsdcMint,
    payer: &Keypair,
    usdc_ata: &Pubkey,
    recent_blockhash: Hash,
) {
    let mint = &usdc_mint.mint;
    let mint_to_ix = mint_to(
        &spl_token::id(),                   // Program ID
        &mint.pubkey(),                     // Mint
        &usdc_ata,                          // User's token account
        &usdc_mint.mint_authority.pubkey(), // Mint authority
        &[],                                // No multisig signers
        1_000_000_000,                      // Amount to mint (e.g., 1000 tokens with 9 decimals)
    )
    .unwrap();

    // Create and sign the transaction
    let mut transaction = Transaction::new_with_payer(&[mint_to_ix], Some(&payer.pubkey()));
    transaction.sign(&[&payer, &usdc_mint.mint_authority], recent_blockhash);

    // Process the transaction
    bank_client.process_transaction(transaction).await.unwrap();
}

async fn create_user_and_mint_usdc(
    bank_client: &mut BanksClient,
    usdc_mint: &UsdcMint,
    payer: &Keypair,
    recent_blockhash: Hash,
) -> User {
    let user_key = Keypair::new();
    let mint = &usdc_mint.mint;
    let user_usdc_ata = get_associated_token_address(&user_key.pubkey(), &mint.pubkey());
    println!("user_usdc_ata={:?}", user_usdc_ata);

    let create_token_account_ix =
        spl_associated_token_account::instruction::create_associated_token_account(
            &payer.pubkey(),    // Fee payer
            &user_key.pubkey(), // Token account owner
            &mint.pubkey(),     // Mint
            &spl_token::id(),   // Token Program PDA
        );

    let mint_to_ix = mint_to(
        &spl_token::id(),                   // Program ID
        &mint.pubkey(),                     // Mint
        &user_usdc_ata,                     // User's token account
        &usdc_mint.mint_authority.pubkey(), // Mint authority
        &[],                                // No multisig signers
        1_000_000_000,                      // Amount to mint (e.g., 1000 tokens with 9 decimals)
    )
    .unwrap();

    // Create and sign the transaction
    let mut transaction = Transaction::new_with_payer(
        &[create_token_account_ix, mint_to_ix],
        Some(&payer.pubkey()),
    );
    transaction.sign(&[&payer, &usdc_mint.mint_authority], recent_blockhash);

    // Process the transaction
    bank_client.process_transaction(transaction).await.unwrap();

    User {
        user_key,
        user_usdc_ata,
    }
}

async fn get_approval(
    bank_client: &mut BanksClient,
    program_id: &Pubkey,
    payer: &Keypair,
    user: &User,
    amount: u64,
    recent_blockhash: Hash,
) {
    let usdc_token_account = &user.user_usdc_ata;
    let (delegate_account, _) = Pubkey::find_program_address(&[b"money"], &program_id);
    let owner = &user.user_key.pubkey();

    let ix = approve(
        &spl_token::id(),
        &usdc_token_account,
        &delegate_account,
        &owner,
        &[],
        amount, // Amount to approve
    )
    .unwrap();

    let mut tx = Transaction::new_with_payer(&[ix], Some(&payer.pubkey()));
    tx.sign(&[&payer, &user.user_key], recent_blockhash);

    // Process the transaction
    bank_client.process_transaction(tx).await.unwrap();
}

async fn initialize_event(
    bank_client: &mut BanksClient,
    payer: &Keypair,
    event_id: u64,
    commission_rate: u64,
    event_total_price: u64,
    program_id: &Pubkey,
    recent_blockhash: Hash,
    usdc_mint: &UsdcMint,
) {
    let data = solana_ctf::InitEventParams {
        event_id,
        commission_rate,
        event_total_price,
    };
    let event_id = data.event_id.to_le_bytes();
    let (event_data_pda, _) =
        Pubkey::find_program_address(&[b"eid_", event_id.as_ref()], program_id);

    let (escrow_pda, _) =
        Pubkey::find_program_address(&[b"usdc_eid_", event_id.as_ref()], program_id);

    let event_account = solana_ctf::accounts::InitializeEvent {
        event_data: event_data_pda,
        payer: payer.pubkey(),
        rent: SYSVAR_RENT_PUBKEY,
        system_program: system_program::id(),
        token_program: OLD_TOKEN_PROGRAM_ID,
        associated_token_program: spl_associated_token_account::id(),
        usdc_mint: usdc_mint.mint.pubkey(),
        escrow_account: escrow_pda,
        delegate: escrow_pda,
    };
    let ix = solana_ctf::instruction::InitializeEvent { data };

    let create_event_ix = Instruction {
        program_id: program_id.clone(),
        accounts: event_account.to_account_metas(None),
        data: ix.data(),
    };

    let mut transaction = Transaction::new_with_payer(
        &[create_event_ix],    // Include the instruction
        Some(&payer.pubkey()), // Specify the fee payer
    );

    transaction.sign(&[&payer], recent_blockhash);

    bank_client.process_transaction(transaction).await.unwrap();
}

async fn create_mints(
    bank_client: &mut BanksClient,
    payer: &Keypair,
    event_id: u64,
    program_id: &Pubkey,
    recent_blockhash: Hash,
    tick_size: u64,
    mint_authority: &Pubkey,
) {
    let mut start = tick_size;

    while start < ONE_DOLLAR {
        let mut create_mints_ix = vec![];
        for token_type in solana_ctf::TokenType::iterator() {
            let data = solana_ctf::CreateMintParams {
                event_id,
                token_type: token_type.clone(),
                token_price: start,
                mint_authority: mint_authority.clone(),
            };

            let (mint_pda, _) = Pubkey::find_program_address(
                &[
                    b"eid_",
                    data.event_id.to_le_bytes().as_ref(),
                    b"_tt_",
                    &[data.token_type.clone() as u8],
                    b"_tp_",
                    data.token_price.to_le_bytes().as_ref(),
                ],
                &program_id,
            );

            let event_account = solana_ctf::accounts::CreateMintData {
                mint: mint_pda,
                payer: payer.pubkey(),
                rent: SYSVAR_RENT_PUBKEY,
                system_program: system_program::id(),
                token_program: TOKEN_PROGRAM_ID,
            };
            let ix = solana_ctf::instruction::CreateMint { params: data };

            let create_mint_ix = Instruction {
                program_id: program_id.clone(),
                accounts: event_account.to_account_metas(None),
                data: ix.data(),
            };

            create_mints_ix.push(create_mint_ix.clone());
        }
        let mut transaction = Transaction::new_with_payer(
            &create_mints_ix,      // Include the instruction
            Some(&payer.pubkey()), // Specify the fee payer
        );

        transaction.sign(&[&payer], recent_blockhash);

        bank_client.process_transaction(transaction).await.unwrap();
        start += tick_size;
    }
}

async fn buy_token(
    bank_client: &mut BanksClient,
    payer: &Keypair,
    event_id: u64,
    program_id: &Pubkey,
    recent_blockhash: Hash,
    mint_authority: &Pubkey,
    token_type: solana_ctf::TokenType,
    token_price: u64,
    user_id: u64,
    quantity: u64,
    user: &User,
    arka_usdc_ata: &Pubkey,
) {
    let data = solana_ctf::MintTokenParams {
        token_type,
        token_price,
        event_id,
        quantity,
        user_id,
    };

    let event_id = data.event_id.to_le_bytes();
    let (event_data_pda, _) =
        Pubkey::find_program_address(&[b"eid_", event_id.as_ref()], program_id);

    let (mint_pda, _) = Pubkey::find_program_address(
        &[
            b"eid_",
            data.event_id.to_le_bytes().as_ref(),
            b"_tt_",
            &[data.token_type.clone() as u8],
            b"_tp_",
            data.token_price.to_le_bytes().as_ref(),
        ],
        &program_id,
    );

    let uid = data.user_id.to_le_bytes();
    let eid = data.event_id.to_le_bytes();
    let tp = data.token_price.to_le_bytes();
    let user_seed = &[
        b"uid_",
        uid.as_ref(),
        b"_eid_",
        eid.as_ref(),
        b"_tt_",
        &[data.token_type.clone() as u8],
        b"_tp_",
        tp.as_ref(),
    ];

    let (user_arka_token_account_pda, _) = Pubkey::find_program_address(user_seed, &program_id);
    let (delegate_account, _) = Pubkey::find_program_address(&[b"money"], &program_id);

    let accounts = solana_ctf::accounts::MintTokens {
        arka_mint: mint_pda,
        user_arka_token_account: user_arka_token_account_pda,
        user_usdc_token_account: user.user_usdc_ata.clone(),
        arka_usdc_event_token_account: arka_usdc_ata.clone(),
        payer: payer.pubkey(),
        rent: SYSVAR_RENT_PUBKEY,
        system_program: system_program::id(),
        token_program: TOKEN_PROGRAM_ID,
        old_token_program: OLD_TOKEN_PROGRAM_ID,
        associated_token_program: spl_associated_token_account::id(),
        authority: mint_authority.clone(),
        delegate: delegate_account,
        event_data: event_data_pda,
    };

    let ix = solana_ctf::instruction::MintTokens { params: data };

    let buy_token_ix = Instruction {
        program_id: program_id.clone(),
        accounts: accounts.to_account_metas(None),
        data: ix.data(),
    };

    let mut transaction = Transaction::new_with_payer(
        &[buy_token_ix],       // Include the instruction
        Some(&payer.pubkey()), // Specify the fee payer
    );

    transaction.sign(&[&payer], recent_blockhash);

    bank_client.process_transaction(transaction).await.unwrap();
}

async fn sell_token(
    bank_client: &mut BanksClient,
    payer: &Keypair,
    event_id: u64,
    program_id: &Pubkey,
    recent_blockhash: Hash,
    mint_authority: &Pubkey,
    token_type: solana_ctf::TokenType,
    token_price: u64,
    user_id: u64,
    quantity: u64,
    user: &User,
    arka_event_usdc_ata: &Pubkey,
    arka_usdc_ata: &Pubkey,
    selling_price: u64,
) {
    let data = solana_ctf::BurnTokenParams {
        token_type,
        token_price,
        event_id,
        quantity,
        user_id,
        selling_price,
    };

    let event_id = data.event_id.to_le_bytes();
    let (event_data_pda, _) =
        Pubkey::find_program_address(&[b"eid_", event_id.as_ref()], program_id);

    let (mint_pda, _) = Pubkey::find_program_address(
        &[
            b"eid_",
            data.event_id.to_le_bytes().as_ref(),
            b"_tt_",
            &[data.token_type.clone() as u8],
            b"_tp_",
            data.token_price.to_le_bytes().as_ref(),
        ],
        &program_id,
    );

    let uid = data.user_id.to_le_bytes();
    let eid = data.event_id.to_le_bytes();
    let tp = data.token_price.to_le_bytes();
    let user_seed = &[
        b"uid_",
        uid.as_ref(),
        b"_eid_",
        eid.as_ref(),
        b"_tt_",
        &[data.token_type.clone() as u8],
        b"_tp_",
        tp.as_ref(),
    ];

    let (user_arka_token_account_pda, _) = Pubkey::find_program_address(user_seed, &program_id);
    let (delegate_account, _) =
        Pubkey::find_program_address(&[b"usdc_eid_", eid.as_ref()], &program_id);

    let accounts = solana_ctf::accounts::BurnTokens {
        arka_mint: mint_pda,
        user_arka_token_account: user_arka_token_account_pda,
        user_usdc_token_account: user.user_usdc_ata.clone(),
        arka_usdc_event_token_account: arka_event_usdc_ata.clone(),
        arka_usdc_token_account: arka_usdc_ata.clone(),
        payer: payer.pubkey(),
        rent: SYSVAR_RENT_PUBKEY,
        system_program: system_program::id(),
        token_program: TOKEN_PROGRAM_ID,
        old_token_program: OLD_TOKEN_PROGRAM_ID,
        associated_token_program: spl_associated_token_account::id(),
        authority: mint_authority.clone(),
        delegate: delegate_account,
        event_data: event_data_pda,
    };

    let ix = solana_ctf::instruction::BurnTokens { params: data };

    let buy_token_ix = Instruction {
        program_id: program_id.clone(),
        accounts: accounts.to_account_metas(None),
        data: ix.data(),
    };

    let mut transaction = Transaction::new_with_payer(
        &[buy_token_ix],       // Include the instruction
        Some(&payer.pubkey()), // Specify the fee payer
    );

    transaction.sign(&[&payer], recent_blockhash);

    bank_client.process_transaction(transaction).await.unwrap();
}

#[tokio::test]
async fn test_program() {
    let program_id = Pubkey::from_str("EEvREcEYzAV31rNmw2QGJHQw54Gbc39vov2fbfCFf7PF").unwrap();
    let program_test = ProgramTest::new("solana_ctf", program_id, None);
    let (mut banks_client, payer, recent_blockhash) = program_test.start().await;
    let usdc_mint = create_usdc_mint(&mut banks_client, &payer, recent_blockhash).await;

    println!("USDC mint pubkey={:?}", usdc_mint.mint.pubkey());

    let event_id: u64 = 1;
    let event_id_bytes = event_id.to_le_bytes();

    initialize_event(
        &mut banks_client,
        &payer,
        event_id,
        10,
        1000_000,
        &program_id,
        recent_blockhash,
        &usdc_mint,
    )
    .await;

    let (arka_event_usdc_account_ata, _) =
        Pubkey::find_program_address(&[b"usdc_eid_", event_id_bytes.as_ref()], &program_id);

    println!("arka_usdc_ata: {:?}", arka_event_usdc_account_ata);

    get_usdc_account(&mut banks_client, &arka_event_usdc_account_ata).await;

    mint_usdc(
        &mut banks_client,
        &usdc_mint,
        &payer,
        &arka_event_usdc_account_ata,
        recent_blockhash,
    )
    .await;

    get_usdc_account(&mut banks_client, &arka_event_usdc_account_ata).await;

    let arka_usdc_account =
        create_user_and_mint_usdc(&mut banks_client, &usdc_mint, &payer, recent_blockhash).await;

    let user1 =
        create_user_and_mint_usdc(&mut banks_client, &usdc_mint, &payer, recent_blockhash).await;
    get_approval(
        &mut banks_client,
        &program_id,
        &payer,
        &user1,
        900000,
        recent_blockhash,
    )
    .await;
    get_usdc_account(&mut banks_client, &user1.user_usdc_ata).await;

    let user2 =
        create_user_and_mint_usdc(&mut banks_client, &usdc_mint, &payer, recent_blockhash).await;
    get_approval(
        &mut banks_client,
        &program_id,
        &payer,
        &user2,
        100,
        recent_blockhash,
    )
    .await;
    get_usdc_account(&mut banks_client, &user2.user_usdc_ata).await;

    create_mints(
        &mut banks_client,
        &payer,
        1,
        &program_id,
        recent_blockhash,
        100000,
        &payer.pubkey(),
    )
    .await;

    buy_token(
        &mut banks_client,
        &payer,
        1,
        &program_id,
        recent_blockhash,
        &payer.pubkey(),
        solana_ctf::TokenType::Yes,
        300000,
        1,
        3,
        &user1,
        &arka_event_usdc_account_ata,
    )
    .await;

    get_usdc_account(&mut banks_client, &user1.user_usdc_ata).await;
    get_usdc_account(&mut banks_client, &arka_event_usdc_account_ata).await;

    sell_token(
        &mut banks_client,
        &payer,
        1,
        &program_id,
        recent_blockhash,
        &payer.pubkey(),
        solana_ctf::TokenType::Yes,
        300000,
        1,
        3,
        &user1,
        &arka_event_usdc_account_ata,
        &arka_usdc_account.user_usdc_ata,
        500000,
    )
    .await;

    get_usdc_account(&mut banks_client, &user1.user_usdc_ata).await;
    get_usdc_account(&mut banks_client, &arka_event_usdc_account_ata).await;
    get_usdc_account(&mut banks_client, &arka_usdc_account.user_usdc_ata).await;

    assert!(false);
}
