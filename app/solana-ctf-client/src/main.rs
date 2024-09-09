use anchor_client::solana_sdk::account::Account;
use anchor_client::{
    solana_sdk::{
        commitment_config,
        pubkey::Pubkey,
        signature::{read_keypair_file, Keypair},
        signer::Signer,
        system_program,
        sysvar::rent::ID as SYSVAR_RENT_PUBKEY,
        transaction::Transaction,
    },
    Client, Cluster, Program,
};
use anchor_lang::Key;
// use anchor_lang::{prelude::AccountLoader, Key};
use anchor_spl::token::{Mint, ID as TOKEN_PROGRAM_ID};
// use core::slice::SlicePattern;
use solana_client::rpc_client::RpcClient;
use solana_ctf::{accounts, instruction, EventOutcome, MintTokenParams};
use spl_associated_token_account::get_associated_token_address;
use spl_token::instruction::approve;
use spl_token::solana_program::program_pack::Pack;
use spl_token::state::Account as SplTokenAccount;
use std::str::FromStr; // Import the SPL Token Account struct

fn approve_transfer() -> Result<(), Box<dyn std::error::Error>> {
    let client = RpcClient::new("http://localhost:8899");
    let payer = read_keypair_file(&*shellexpand::tilde("~/event-accounts/event-2.json"))?;

    // let source_token_account = Pubkey::from_str("C3jFyfw78Pir9iQsHuHxeCRFLFrZmQQz5BrfJJS8DD8d")?;
    // let delegate_account = Pubkey::from_str("EMSsxw9k6kFPp2F4XMdp87q9NwDvAbkMYdRo9BzV1VbP")?;
    // let owner = Pubkey::from_str("6FXCDnDAQcUWMtmnwSBQ3NukooLPBfGuwcH586cX3vec").unwrap();

    // let ix = approve(
    //     &spl_token::id(),
    //     &source_token_account,
    //     &delegate_account,
    //     &owner,
    //     &[],
    //     10000, // Amount to approve
    // )?;

    // let mut tx = Transaction::new_with_payer(&[ix], Some(&payer.pubkey()));
    // let recent_blockhash = client.get_latest_blockhash()?;
    // tx.sign(&[&payer], recent_blockhash);

    // client.send_and_confirm_transaction(&tx)?;

    let probo_usdc_token_account =
        Pubkey::from_str("9FQ6qkx9jh3FALxGz4P14tLbepPDZQhUN3fgDccgzL5e").unwrap();
    let account_info = client.get_token_account(&probo_usdc_token_account);
    println!("{:?}", account_info);

    Ok(())
}

fn create_event(payer: &Keypair, program: &Program<&Keypair>, program_id: &Pubkey) {
    let params = solana_ctf::InitEventParams {
        event_id: 1,
        commission_rate: 1,
    };
    let event_id = params.event_id.to_le_bytes();
    let (event_data_pda, _) =
        Pubkey::find_program_address(&[b"eid_", event_id.as_ref()], program_id);
    let event_context = accounts::InitializeEvent {
        event_data: event_data_pda,
        payer: payer.pubkey(),
        system_program: system_program::id(),
    };

    program
        .request()
        .accounts(event_context)
        .args(instruction::InitializeEvent { data: params })
        .signer(&payer)
        .send()
        .unwrap();
}

fn create_mint(
    payer: &Keypair,
    program: &Program<&Keypair>,
    program_id: &Pubkey,
    params: solana_ctf::CreateMintParams,
) {
    let (minting_pda, _) = Pubkey::find_program_address(
        &[
            b"eid_",
            params.event_id.to_le_bytes().as_ref(),
            b"_tt_",
            &[params.token_type.clone() as u8],
            b"_tp_",
            params.token_price.to_le_bytes().as_ref(),
        ],
        &program_id,
    );

    let mint_context = accounts::CreateMintData {
        mint: minting_pda,
        payer: payer.pubkey(),
        rent: SYSVAR_RENT_PUBKEY,
        system_program: system_program::id(),
        token_program: TOKEN_PROGRAM_ID,
    };

    program
        .request()
        .accounts(mint_context)
        .args(instruction::CreateMint { params })
        .signer(&payer)
        .send()
        .unwrap();
}

fn buy_token_workflow(payer: &Keypair, program: &Program<&Keypair>, program_id: &Pubkey) {
    // let x = {
    //     probo_mint: 9Ma2ccKCPKdCZ1GhVP2TvZwLR9UFfBCQsiyecpkgsBK3,
    //     user_probo_token_account: 2TRVLrzSAPehWbcRyuuLWVF51MJwTYgvfhxKGmqXog19,
    //     user_usdc_token_account: 4YRevpgzSR2oixxDJvySzZk6sYbryxQcaNywNHesv9EY,
    //     probo_usdc_token_account: 9FQ6qkx9jh3FALxGz4P14tLbepPDZQhUN3fgDccgzL5e,
    //     payer: 2bLb3xNuxZzHFHAWa2utZTFdqzrVHpfjHnpyvumt8GAv,
    //     rent: SysvarRent111111111111111111111111111111111,
    //     system_program: 11111111111111111111111111111111,
    //     token_program: TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA,
    //     associated_token_program: ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL,
    //     authority: 2bLb3xNuxZzHFHAWa2utZTFdqzrVHpfjHnpyvumt8GAv,
    //     delegate: FriF2CGsHKL2DGpL4qWEpzNKv2v1saQqA9g85krZHA99
    // }

    let usdc_mint_pubkey =
        Pubkey::from_str("5TiXaoC1n5h1aSMoyYDnZrsxGC3VFhEvHZvcTYCGVg4E").unwrap();
    let user_usdc_token_account =
        Pubkey::from_str("4YRevpgzSR2oixxDJvySzZk6sYbryxQcaNywNHesv9EY").unwrap(); //get_associated_token_address(&payer.pubkey(), &usdc_mint_pubkey);
    let arka_usdc_token_account =
        Pubkey::from_str("9FQ6qkx9jh3FALxGz4P14tLbepPDZQhUN3fgDccgzL5e").unwrap();
    let (delegate_account, _) = Pubkey::find_program_address(&[b"money"], &program_id);
    println!("Delegate: {}", delegate_account);
    let mint_data = MintTokenParams {
        token_type: solana_ctf::TokenType::Yes,
        token_price: 31,
        event_id: 3221,
        quantity: 1,
        user_id: 213,
    };
    let (arka_mint_pda, arka_mint_bumps) = Pubkey::find_program_address(
        &[
            b"eid_",
            mint_data.event_id.to_le_bytes().as_ref(),
            b"_tt_",
            &[mint_data.token_type.clone() as u8],
            b"_tp_",
            mint_data.token_price.to_le_bytes().as_ref(),
        ],
        program_id,
    );

    let user_id = mint_data.user_id.to_le_bytes();
    let event_id = mint_data.event_id.to_le_bytes();
    let tp = mint_data.token_price.to_le_bytes();
    let user_seed = &[
        b"uid_",
        user_id.as_ref(),
        b"_eid_",
        event_id.as_ref(),
        b"_tt_",
        &[mint_data.token_type.clone() as u8],
        b"_tp_",
        tp.as_ref(),
    ];
    let (user_arka_token_account_pda, bumps) = Pubkey::find_program_address(user_seed, &program_id);
    // let user_probo_token_account_pda =
    //     get_associated_token_address(&payer.pubkey(), &probo_mint_pda);

    println!(
        "user: {:?} bumps: {:?} seed: {:?} prob_mint: {:?}",
        user_arka_token_account_pda, arka_mint_bumps, user_seed, arka_mint_pda
    );

    let context = accounts::MintTokens {
        arka_mint: arka_mint_pda,
        user_arka_token_account: user_arka_token_account_pda,
        user_usdc_token_account,
        arka_usdc_token_account,
        payer: payer.pubkey(),
        rent: SYSVAR_RENT_PUBKEY,
        system_program: system_program::id(),
        token_program: TOKEN_PROGRAM_ID,
        associated_token_program: spl_associated_token_account::id(),
        authority: payer.pubkey(),
        delegate: delegate_account,
    };

    program
        .request()
        .accounts(context)
        .args(instruction::MintTokens { params: mint_data })
        .signer(&payer)
        .send()
        .unwrap();
}

fn sell_token_workflow(payer: &Keypair, program: &Program<&Keypair>, program_id: &Pubkey) {
    let usdc_mint_pubkey =
        Pubkey::from_str("5TiXaoC1n5h1aSMoyYDnZrsxGC3VFhEvHZvcTYCGVg4E").unwrap();
    let user_usdc_token_account = get_associated_token_address(&payer.pubkey(), &usdc_mint_pubkey);
    let arka_usdc_token_account =
        Pubkey::from_str("C3jFyfw78Pir9iQsHuHxeCRFLFrZmQQz5BrfJJS8DD8d").unwrap();
    let (delegate_account, _) = Pubkey::find_program_address(&[b"money"], &program_id);

    println!(
        "user_usdc: {:?}, arka_usdc: {:?}",
        user_usdc_token_account, arka_usdc_token_account
    );

    let burn_data = solana_ctf::BurnTokenParams {
        token_type: solana_ctf::TokenType::Yes,
        token_price: 31,
        event_id: 3221,
        quantity: 1,
        user_id: 213,
        selling_price: 41,
    };
    let (arka_mint_pda, arka_mint_bumps) = Pubkey::find_program_address(
        &[
            b"eid_",
            burn_data.event_id.to_le_bytes().as_ref(),
            b"_tt_",
            &[burn_data.token_type.clone() as u8],
            b"_tp_",
            burn_data.token_price.to_le_bytes().as_ref(),
        ],
        program_id,
    );

    let user_id = burn_data.user_id.to_le_bytes();
    let event_id = burn_data.event_id.to_le_bytes();
    let tp = burn_data.token_price.to_le_bytes();
    let user_seed = &[
        b"uid_",
        user_id.as_ref(),
        b"_eid_",
        event_id.as_ref(),
        b"_tt_",
        &[burn_data.token_type.clone() as u8],
        b"_tp_",
        tp.as_ref(),
    ];
    let (user_arka_token_account_pda, bumps) = Pubkey::find_program_address(user_seed, &program_id);

    let (event_data_pda, _) =
        Pubkey::find_program_address(&[b"eid_", event_id.as_ref()], program_id);

    println!(
        "user: {:?} bumps: {:?} seed: {:?} prob_mint: {:?}",
        user_arka_token_account_pda, arka_mint_bumps, user_seed, arka_mint_pda
    );

    let context = accounts::BurnTokens {
        arka_mint: arka_mint_pda,
        user_arka_token_account: user_arka_token_account_pda,
        user_usdc_token_account,
        arka_usdc_token_account,
        payer: payer.pubkey(),
        rent: SYSVAR_RENT_PUBKEY,
        system_program: system_program::id(),
        token_program: TOKEN_PROGRAM_ID,
        associated_token_program: spl_associated_token_account::id(),
        delegate: delegate_account,
        authority: payer.pubkey(),
        event_data: event_data_pda,
    };

    program
        .request()
        .accounts(context)
        .args(instruction::BurnTokens { params: burn_data })
        .signer(&payer)
        .send()
        .unwrap();
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let payer = read_keypair_file(&*shellexpand::tilde("~/event-accounts/event-2.json"))?;
    let url = Cluster::Custom(
        "http://localhost:8899".to_string(),
        "ws://127.0.0.1:8900".to_string(),
    );
    let client = Client::new_with_options(
        url.clone(),
        &payer,
        commitment_config::CommitmentConfig::processed(),
    );

    // assert_eq!(account_info.delegate, Some(program_pda));

    let program = client.program(Pubkey::from_str(
        "EMSsxw9k6kFPp2F4XMdp87q9NwDvAbkMYdRo9BzV1VbP",
    )?)?;
    let program_id = Pubkey::from_str("EMSsxw9k6kFPp2F4XMdp87q9NwDvAbkMYdRo9BzV1VbP")?;

    // program
    //     .request()
    //     .accounts(account)
    //     .args(instruction::InitToken { metadata })
    //     .signer(&payer)
    //     .send()?;
    // Define the token account address
    // let token_account_pubkey = Pubkey::from_str("YourTokenAccountPubkeyHere")?;

    let sender = Pubkey::from_str("4RnsVUhvYMWo85JszsJCc2zFkYgEBfQnmr2Qvuyx5cS1").unwrap();
    let receiver = Pubkey::from_str("8m9eHmecgvGmXq9htTy3qRs8AtmMJX1niYoMd3g36Kma").unwrap();
    let token_program = Pubkey::from_str("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA").unwrap();
    let auth = Pubkey::from_str("CvDDtL71MNzoTjBdqsbeZnvd3vXFkKvYyRfHHoaGoDom").unwrap();

    println!("Cominghere");

    // program
    //     .request()
    //     .accounts(tcontext)
    //     .args(instruction::TransferTokens { amount: 10000 })
    //     .signer(&payer)
    //     .send()?;

    // let rpc_url = "https://api.mainnet-"; // Use the appropriate cluster URL
    // let rpc_client = RpcClient::new(rpc_url);
    // // Fetch the token account data
    // let account_info = rpc_client.get_account_data(&destination)?;

    // // Deserialize the account data
    // let token_account = TokenAccount::unpack(&account_info)?;

    // Print the balance
    // println!("Token Account Balance: {:?}", destination);

    // let account_info = program.account::<TokenAccount>(destination).unwrap();

    // println!("Token Account Balance: {:?}", account_info);
    //
    // let rpc_client = RpcClient::new("http://localhost:8899".to_string()); // Replace with your Solana cluster URL
    // let account_data = rpc_client.get_account_data(&destination)?;

    // let token_account = SplTokenAccount::unpack(&account_data)?;

    // println!("Num tokens: {:?}", token_account.amount);

    // approve_transfer().unwrap();

    // println!("Approved txns!");
    //
    // approve_transfer().unwrap();
    // sell_token_workflow(&payer, &program, &program_id);
    // buy_token_workflow(&payer, &program, &program_id);
    //  create_event(&payer, &program, &program_id);
    //create_mint(payer, program, program_id,

    for i in 0..10 {
        create_mint(
            &payer,
            &program,
            &program_id,
            solana_ctf::CreateMintParams {
                event_id: 3,
                token_type: solana_ctf::TokenType::Yes,
                token_price: i * 100,
                mint_authority: payer.pubkey(),
            },
        );
        create_mint(
            &payer,
            &program,
            &program_id,
            solana_ctf::CreateMintParams {
                event_id: 3,
                token_type: solana_ctf::TokenType::No,
                token_price: i * 100,
                mint_authority: payer.pubkey(),
            },
        );
    }

    Ok(())
}
