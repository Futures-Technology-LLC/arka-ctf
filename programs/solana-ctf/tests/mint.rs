use std::str::FromStr;

use anchor_lang::prelude::*;
use anchor_lang::InstructionData;
use anchor_spl::token::TokenAccount;
use anchor_spl::token::ID as OLD_TOKEN_PROGRAM_ID;
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
use std::fs::File;
use std::io::Read;

pub const ONE_DOLLAR: u64 = 1_000_000;
pub const OWNER: Pubkey = Pubkey::new_from_array([
    148, 244, 35, 255, 110, 248, 40, 221, 236, 11, 199, 213, 242, 243, 97, 161, 22, 80, 148, 47,
    144, 114, 254, 166, 91, 138, 193, 71, 72, 37, 36, 148,
]);

fn load_keypair_from_file(file_path: &str) -> Keypair {
    // Read the JSON file
    let mut file = File::open(file_path).unwrap();
    let mut data = String::new();
    file.read_to_string(&mut data).unwrap();

    // Parse the JSON into a Vec<u8>
    let keypair_vec: Vec<u8> = serde_json::from_str(&data).unwrap();

    // Convert the Vec<u8> into a Keypair
    let keypair = Keypair::from_bytes(&keypair_vec)
        .map_err(|_| "Failed to create Keypair from the provided file")
        .unwrap();

    keypair
}

pub struct UsdcMint {
    pub mint: Keypair,
    pub mint_authority: Keypair,
}

// #[derive(Debug)]
pub struct User {
    pub user_key: Keypair,
    pub user_usdc_ata: Pubkey,
}

async fn close_event_account(
    bank_client: &mut BanksClient,
    payer: &Keypair,
    recent_blockhash: Hash,
    program_id: &Pubkey,
    event_id: u64,
    keypair: &Keypair,
) {
    let data = solana_ctf::CloseEventAccountParams { event_id };
    let event_id = event_id.to_le_bytes();
    let (event_data_pda, _) =
        Pubkey::find_program_address(&[b"eid_", event_id.as_ref()], program_id);

    let event_account = solana_ctf::accounts::CloseEventAccount {
        owner: OWNER,
        event_data: event_data_pda,
        payer: payer.pubkey(),
    };
    let ix = solana_ctf::instruction::CloseEventData { params: data };

    let create_event_ix = Instruction {
        program_id: program_id.clone(),
        accounts: event_account.to_account_metas(None),
        data: ix.data(),
    };

    let mut transaction = Transaction::new_with_payer(
        &[create_event_ix],    // Include the instruction
        Some(&payer.pubkey()), // Specify the fee payer
    );

    transaction.sign(&[&payer, keypair], recent_blockhash);

    bank_client.process_transaction(transaction).await.unwrap();
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
    event_total_price: u64,
    program_id: &Pubkey,
    recent_blockhash: Hash,
    usdc_mint: &UsdcMint,
    keypair: &Keypair,
) {
    let data = solana_ctf::InitEventParams {
        event_id,
        event_total_price,
    };
    let event_id = data.event_id.to_le_bytes();
    let (event_data_pda, _) =
        Pubkey::find_program_address(&[b"eid_", event_id.as_ref()], program_id);

    let (escrow_pda, _) =
        Pubkey::find_program_address(&[b"usdc_eid_", event_id.as_ref()], program_id);

    let event_account = solana_ctf::accounts::InitializeEvent {
        owner: OWNER,
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

    transaction.sign(&[&payer, keypair], recent_blockhash);

    bank_client.process_transaction(transaction).await.unwrap();
}

async fn initialize_user(
    bank_client: &mut BanksClient,
    payer: &Keypair,
    user_id: u64,
    program_id: &Pubkey,
    recent_blockhash: Hash,
    usdc_mint: &UsdcMint,
    keypair: &Keypair,
    arka_usdc_ata: &Pubkey,
) {
    let data = solana_ctf::InitUserAtaParams {
        user_id,
        promo_balance: 2000000,
    };
    let user_id = data.user_id.to_le_bytes();

    let (escrow_pda, _) =
        Pubkey::find_program_address(&[b"usdc_uid_", user_id.as_ref()], program_id);

    let (promo_pda, _) =
        Pubkey::find_program_address(&[b"promo_usdc_uid_", user_id.as_ref()], program_id);
    let (delegate_account, _) = Pubkey::find_program_address(&[b"money"], &program_id);

    println!(
        "arka_usdc_ata: {:?} {:?} {:?}",
        arka_usdc_ata, escrow_pda, promo_pda
    );
    let event_account = solana_ctf::accounts::InitializeUserAta {
        owner: OWNER,
        payer: payer.pubkey(),
        rent: SYSVAR_RENT_PUBKEY,
        system_program: system_program::id(),
        token_program: OLD_TOKEN_PROGRAM_ID,
        associated_token_program: spl_associated_token_account::id(),
        usdc_mint: usdc_mint.mint.pubkey(),
        escrow_account: escrow_pda,
        delegate: escrow_pda,
        promo_account: promo_pda,
        promo_delegate: promo_pda,
        arka_usdc_wallet: arka_usdc_ata.clone(),
        arka_delegate: delegate_account,
    };
    let ix = solana_ctf::instruction::InitializeUserAta { data };

    let create_user_ix = Instruction {
        program_id: program_id.clone(),
        accounts: event_account.to_account_metas(None),
        data: ix.data(),
    };

    let mut transaction = Transaction::new_with_payer(
        &[create_user_ix],     // Include the instruction
        Some(&payer.pubkey()), // Specify the fee payer
    );

    transaction.sign(&[&payer, keypair], recent_blockhash);

    bank_client.process_transaction(transaction).await.unwrap();
}

async fn transfer_from_user_wallet_to_pda(
    bank_client: &mut BanksClient,
    payer: &Keypair,
    user_id: u64,
    user: &User,
    amount: u64,
    program_id: &Pubkey,
    recent_blockhash: Hash,
    usdc_mint: &UsdcMint,
    keypair: &Keypair,
) {
    let data = solana_ctf::TranferFromUserWalletParams {
        user_id,
        amount,
        event_id: 1,
        order_id: 1,
        promo_amount: 20000,
    };
    let user_id = data.user_id.to_le_bytes();

    let (escrow_pda, _) =
        Pubkey::find_program_address(&[b"usdc_uid_", user_id.as_ref()], program_id);
    let (promo_pda, _) =
        Pubkey::find_program_address(&[b"promo_usdc_uid_", user_id.as_ref()], program_id);
    let (delegate_account, _) = Pubkey::find_program_address(&[b"money"], &program_id);

    let event_account = solana_ctf::accounts::TranferFromUserWallet {
        owner: OWNER,
        payer: payer.pubkey(),
        rent: SYSVAR_RENT_PUBKEY,
        system_program: system_program::id(),
        token_program: OLD_TOKEN_PROGRAM_ID,
        associated_token_program: spl_associated_token_account::id(),
        usdc_mint: usdc_mint.mint.pubkey(),
        escrow_account: escrow_pda,
        delegate: delegate_account,
        user_usdc_token_account: Some(user.user_usdc_ata.clone()),
        promo_account: Some(promo_pda),
        promo_delegate: Some(promo_pda),
    };
    let ix = solana_ctf::instruction::TransferFromUserWalletToPda { data };

    let create_user_ix = Instruction {
        program_id: program_id.clone(),
        accounts: event_account.to_account_metas(None),
        data: ix.data(),
    };

    let mut transaction = Transaction::new_with_payer(
        &[create_user_ix],     // Include the instruction
        Some(&payer.pubkey()), // Specify the fee payer
    );

    transaction.sign(&[&payer, keypair], recent_blockhash);

    bank_client.process_transaction(transaction).await.unwrap();
}

async fn transfer_from_user_pda_to_wallet(
    bank_client: &mut BanksClient,
    payer: &Keypair,
    user_id: u64,
    user: &User,
    amount: u64,
    program_id: &Pubkey,
    recent_blockhash: Hash,
    usdc_mint: &UsdcMint,
) {
    let data = solana_ctf::TranferFromUserPdaParams {
        user_id,
        amount,
        event_id: 1,
        order_id: 1,
        utr_id: "test".to_string(),
        promo_amount: 20000,
    };
    let user_id = data.user_id.to_le_bytes();

    let (escrow_pda, _) =
        Pubkey::find_program_address(&[b"usdc_uid_", user_id.as_ref()], program_id);
    let (promo_pda, _) =
        Pubkey::find_program_address(&[b"promo_usdc_uid_", user_id.as_ref()], program_id);

    let event_account = solana_ctf::accounts::TranferFromUserPda {
        owner: OWNER,
        payer: payer.pubkey(),
        rent: SYSVAR_RENT_PUBKEY,
        system_program: system_program::id(),
        token_program: OLD_TOKEN_PROGRAM_ID,
        associated_token_program: spl_associated_token_account::id(),
        usdc_mint: usdc_mint.mint.pubkey(),
        escrow_account: escrow_pda,
        delegate: escrow_pda,
        user_usdc_token_account: Some(user.user_usdc_ata.clone()),
        promo_account: Some(promo_pda),
    };
    let ix = solana_ctf::instruction::TransferFromUserPdaToWallet { data };

    let create_user_ix = Instruction {
        program_id: program_id.clone(),
        accounts: event_account.to_account_metas(None),
        data: ix.data(),
    };

    let mut transaction = Transaction::new_with_payer(
        &[create_user_ix],     // Include the instruction
        Some(&payer.pubkey()), // Specify the fee payer
    );

    transaction.sign(&[&payer], recent_blockhash);

    bank_client.process_transaction(transaction).await.unwrap();
}

async fn buy_token(
    bank_client: &mut BanksClient,
    payer: &Keypair,
    event_id: u64,
    program_id: &Pubkey,
    recent_blockhash: Hash,
    order_type: solana_ctf::OrderType,
    order_price: u64,
    user_id: u64,
    quantity: u64,
    arka_usdc_ata: &Pubkey,
    keypair: &Keypair,
) {
    let data = solana_ctf::BuyOrderParams {
        order_type,
        order_price,
        event_id,
        quantity,
        user_id,
        commission: 10,
    };

    let event_id = data.event_id.to_le_bytes();
    let (event_data_pda, _) =
        Pubkey::find_program_address(&[b"eid_", event_id.as_ref()], program_id);

    let uid = data.user_id.to_le_bytes();
    let eid = data.event_id.to_le_bytes();
    let user_seed = &[b"uid_", uid.as_ref(), b"_eid_", eid.as_ref()];

    let (user_arka_event_account_pda, _) = Pubkey::find_program_address(user_seed, &program_id);
    let (delegate_account, _) =
        Pubkey::find_program_address(&[b"usdc_uid_", uid.as_ref()], &program_id);

    let accounts = solana_ctf::accounts::BuyOrder {
        owner: OWNER,
        user_arka_event_account: user_arka_event_account_pda,
        user_usdc_token_account: delegate_account,
        arka_usdc_event_token_account: arka_usdc_ata.clone(),
        payer: payer.pubkey(),
        rent: SYSVAR_RENT_PUBKEY,
        system_program: system_program::id(),
        old_token_program: OLD_TOKEN_PROGRAM_ID,
        delegate: delegate_account,
        event_data: event_data_pda,
    };

    let ix = solana_ctf::instruction::BuyOrder { params: data };

    let buy_token_ix = Instruction {
        program_id: program_id.clone(),
        accounts: accounts.to_account_metas(None),
        data: ix.data(),
    };

    let mut transaction = Transaction::new_with_payer(
        &[buy_token_ix],       // Include the instruction
        Some(&payer.pubkey()), // Specify the fee payer
    );

    transaction.sign(&[&payer, keypair], recent_blockhash);

    bank_client.process_transaction(transaction).await.unwrap();
}

async fn sell_token(
    bank_client: &mut BanksClient,
    payer: &Keypair,
    event_id: u64,
    program_id: &Pubkey,
    recent_blockhash: Hash,
    order_type: solana_ctf::OrderType,
    order_price: u64,
    user_id: u64,
    quantity: u64,
    arka_event_usdc_ata: &Pubkey,
    arka_usdc_ata: &Pubkey,
    selling_price: u64,
    keypair: &Keypair,
) {
    let data = solana_ctf::SellOrderParams {
        order_type,
        order_price,
        event_id,
        quantity,
        user_id,
        selling_price,
        promo_amount: 20000,
    };

    let event_id = data.event_id.to_le_bytes();
    let (event_data_pda, _) =
        Pubkey::find_program_address(&[b"eid_", event_id.as_ref()], program_id);

    let uid = data.user_id.to_le_bytes();
    let eid = data.event_id.to_le_bytes();
    let user_seed = &[b"uid_", uid.as_ref(), b"_eid_", eid.as_ref()];

    let (user_arka_event_account_pda, _) = Pubkey::find_program_address(user_seed, &program_id);
    let (delegate_account, _) =
        Pubkey::find_program_address(&[b"usdc_eid_", eid.as_ref()], &program_id);

    let (user_usdc_token_account, _) =
        Pubkey::find_program_address(&[b"usdc_uid_", uid.as_ref()], &program_id);

    let (promo_account, _) =
        Pubkey::find_program_address(&[b"promo_usdc_uid_", uid.as_ref()], &program_id);

    let accounts = solana_ctf::accounts::SellOrder {
        owner: OWNER,
        user_arka_event_account: user_arka_event_account_pda,
        user_usdc_token_account: Some(user_usdc_token_account),
        arka_usdc_event_token_account: arka_event_usdc_ata.clone(),
        arka_usdc_token_account: arka_usdc_ata.clone(),
        payer: payer.pubkey(),
        rent: SYSVAR_RENT_PUBKEY,
        system_program: system_program::id(),
        old_token_program: OLD_TOKEN_PROGRAM_ID,
        delegate: delegate_account,
        event_data: event_data_pda,
        promo_account: Some(promo_account),
    };

    let ix = solana_ctf::instruction::SellOrder { params: data };

    let buy_token_ix = Instruction {
        program_id: program_id.clone(),
        accounts: accounts.to_account_metas(None),
        data: ix.data(),
    };

    let mut transaction = Transaction::new_with_payer(
        &[buy_token_ix],       // Include the instruction
        Some(&payer.pubkey()), // Specify the fee payer
    );

    transaction.sign(&[&payer, keypair], recent_blockhash);

    bank_client.process_transaction(transaction).await.unwrap();
}

#[tokio::test]
async fn test_program() {
    let program_id = Pubkey::from_str("EEvREcEYzAV31rNmw2QGJHQw54Gbc39vov2fbfCFf7PF").unwrap();
    let program_test = ProgramTest::new("solana_ctf", program_id, None);
    let (mut banks_client, payer, recent_blockhash) = program_test.start().await;
    let usdc_mint = create_usdc_mint(&mut banks_client, &payer, recent_blockhash).await;
    let keypair_file = "/Users/jinankjain/dev/arka-ctf/mainnet-owner.json"; // Path to your keypair JSON file
    let keypair = load_keypair_from_file(keypair_file);

    println!("USDC mint pubkey={:?}", usdc_mint.mint.pubkey());

    let event_id: u64 = 1;
    let user_id: u64 = 1;
    let event_id_bytes = event_id.to_le_bytes();

    initialize_event(
        &mut banks_client,
        &payer,
        event_id,
        1000_000,
        &program_id,
        recent_blockhash,
        &usdc_mint,
        &keypair,
    )
    .await;

    let arka_usdc_wallet =
        create_user_and_mint_usdc(&mut banks_client, &usdc_mint, &payer, recent_blockhash).await;
    get_approval(
        &mut banks_client,
        &program_id,
        &payer,
        &arka_usdc_wallet,
        9000000,
        recent_blockhash,
    )
    .await;
    get_usdc_account(&mut banks_client, &arka_usdc_wallet.user_usdc_ata).await;

    initialize_user(
        &mut banks_client,
        &payer,
        user_id,
        &program_id,
        recent_blockhash,
        &usdc_mint,
        &keypair,
        &arka_usdc_wallet.user_usdc_ata,
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

    transfer_from_user_wallet_to_pda(
        &mut banks_client,
        &payer,
        user_id,
        &user1,
        300000 * 3,
        &program_id,
        recent_blockhash,
        &usdc_mint,
        &keypair,
    )
    .await;

    buy_token(
        &mut banks_client,
        &payer,
        event_id,
        &program_id,
        recent_blockhash,
        solana_ctf::OrderType::Yes,
        300000,
        user_id,
        3,
        &arka_event_usdc_account_ata,
        &keypair,
    )
    .await;

    get_usdc_account(&mut banks_client, &user1.user_usdc_ata).await;
    get_usdc_account(&mut banks_client, &arka_event_usdc_account_ata).await;

    sell_token(
        &mut banks_client,
        &payer,
        event_id,
        &program_id,
        recent_blockhash,
        solana_ctf::OrderType::Yes,
        300000,
        user_id,
        2,
        &arka_event_usdc_account_ata,
        &arka_usdc_account.user_usdc_ata,
        500000,
        &keypair,
    )
    .await;

    get_usdc_account(&mut banks_client, &user1.user_usdc_ata).await;
    get_usdc_account(&mut banks_client, &arka_event_usdc_account_ata).await;
    get_usdc_account(&mut banks_client, &arka_usdc_account.user_usdc_ata).await;

    let balance = banks_client
        .get_balance(payer.pubkey().clone())
        .await
        .unwrap();

    println!("Balance before closing {:?}", balance);

    close_event_account(
        &mut banks_client,
        &payer,
        recent_blockhash,
        &program_id,
        event_id,
        &keypair,
    )
    .await;

    let balance = banks_client
        .get_balance(payer.pubkey().clone())
        .await
        .unwrap();

    println!("Balance after closing {:?}", balance);

    assert!(false);
}
