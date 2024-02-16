use std::str::FromStr;

use ore::{
    state::{Bus, Proof, Treasury},
    utils::{AccountDeserialize, Discriminator},
    BUS_ADDRESSES, BUS_COUNT, INITIAL_REWARD_RATE, MINT_ADDRESS, PROOF, TOKEN_DECIMALS, TREASURY,
    TREASURY_ADDRESS,
};
use solana_program::{
    clock::Clock,
    epoch_schedule::DEFAULT_SLOTS_PER_EPOCH,
    keccak::{hashv, Hash as KeccakHash},
    program_option::COption,
    program_pack::Pack,
    pubkey::Pubkey,
    sysvar,
};
use solana_program_test::{processor, BanksClient, ProgramTest};
use solana_sdk::{
    signature::{Keypair, Signer},
    transaction::Transaction,
};
use spl_token::state::{AccountState, Mint};

#[tokio::test]
async fn test_mine() {
    // Setup
    let (mut banks, payer, hash) = setup_program_test_env().await;

    // Build register ix
    let proof_pda = Pubkey::find_program_address(&[PROOF, payer.pubkey().as_ref()], &ore::id());
    let ix_0 = ore::instruction::register(payer.pubkey());

    // Submit tx
    let tx = Transaction::new_signed_with_payer(&[ix_0], Some(&payer.pubkey()), &[&payer], hash);
    let res = banks.process_transaction(tx).await;
    assert!(res.is_ok());

    // Assert proof state
    let proof_account = banks.get_account(proof_pda.0).await.unwrap().unwrap();
    assert_eq!(proof_account.owner, ore::id());
    let proof = Proof::try_from_bytes(&proof_account.data).unwrap();

    // Assert proof state
    let treasury_pda = Pubkey::find_program_address(&[TREASURY], &ore::id());
    let treasury_account = banks.get_account(treasury_pda.0).await.unwrap().unwrap();
    let treasury = Treasury::try_from_bytes(&treasury_account.data).unwrap();

    // Find next hash
    let (next_hash, nonce) = find_next_hash(
        proof.hash.into(),
        treasury.difficulty.into(),
        payer.pubkey(),
    );

    // Submit mine tx
    let ix_1 = ore::instruction::mine(payer.pubkey(), BUS_ADDRESSES[0], next_hash.into(), nonce);
    let tx = Transaction::new_signed_with_payer(&[ix_1], Some(&payer.pubkey()), &[&payer], hash);
    let res = banks.process_transaction(tx).await;
    assert!(res.is_ok());

    // TODO Assert proof state
    // TODO Assert bus state
}

fn find_next_hash(hash: KeccakHash, difficulty: KeccakHash, signer: Pubkey) -> (KeccakHash, u64) {
    let mut next_hash: KeccakHash;
    let mut nonce = 0u64;
    loop {
        next_hash = hashv(&[
            hash.to_bytes().as_slice(),
            signer.to_bytes().as_slice(),
            nonce.to_be_bytes().as_slice(),
        ]);
        if next_hash.le(&difficulty) {
            break;
        } else {
            println!("Invalid hash: {} Nonce: {:?}", next_hash.to_string(), nonce);
        }
        nonce += 1;
    }
    (next_hash, nonce)
}

async fn setup_program_test_env() -> (BanksClient, Keypair, solana_program::hash::Hash) {
    let mut program_test = ProgramTest::new("ore", ore::ID, processor!(ore::process_instruction));
    program_test.prefer_bpf(true);

    // Busses
    for i in 0..BUS_COUNT {
        program_test.add_account_with_base64_data(
            BUS_ADDRESSES[i],
            1057920,
            ore::id(),
            bs64::encode(
                &[
                    &(Bus::discriminator() as u64).to_le_bytes(),
                    Bus {
                        id: i as u64,
                        rewards: 250_000_000,
                    }
                    .to_bytes(),
                ]
                .concat(),
            )
            .as_str(),
        );
    }

    // Treasury
    let admin_address = Pubkey::from_str("AeNqnoLwFanMd3ig9WoMxQZVwQHtCtqKMMBsT1sTrvz6").unwrap();
    let treasury_pda = Pubkey::find_program_address(&[TREASURY], &ore::id());
    program_test.add_account_with_base64_data(
        treasury_pda.0,
        1614720,
        ore::id(),
        bs64::encode(
            &[
                &(Treasury::discriminator() as u64).to_le_bytes(),
                Treasury {
                    bump: treasury_pda.1 as u64,
                    admin: admin_address,
                    difficulty: KeccakHash::new_from_array([u8::MAX; 32]).into(),
                    epoch_start_at: 100,
                    reward_rate: INITIAL_REWARD_RATE,
                    total_claimed_rewards: 0,
                }
                .to_bytes(),
            ]
            .concat(),
        )
        .as_str(),
    );

    // Mint
    let mut mint_src: [u8; Mint::LEN] = [0; Mint::LEN];
    Mint {
        mint_authority: COption::Some(TREASURY_ADDRESS),
        supply: 2_000_000_000,
        decimals: TOKEN_DECIMALS,
        is_initialized: true,
        freeze_authority: COption::None,
    }
    .pack_into_slice(&mut mint_src);
    program_test.add_account_with_base64_data(
        MINT_ADDRESS,
        1461600,
        spl_token::id(),
        bs64::encode(&mint_src).as_str(),
    );

    // Treasury tokens
    let tokens_address = spl_associated_token_account::get_associated_token_address(
        &TREASURY_ADDRESS,
        &MINT_ADDRESS,
    );
    let mut tokens_src: [u8; spl_token::state::Account::LEN] = [0; spl_token::state::Account::LEN];
    spl_token::state::Account {
        mint: MINT_ADDRESS,
        owner: TREASURY_ADDRESS,
        amount: 2_000_000_000,
        delegate: COption::None,
        state: AccountState::Initialized,
        is_native: COption::None,
        delegated_amount: 0,
        close_authority: COption::None,
    }
    .pack_into_slice(&mut tokens_src);
    program_test.add_account_with_base64_data(
        tokens_address,
        2039280,
        spl_token::id(),
        bs64::encode(&tokens_src).as_str(),
    );

    // Set sysvar
    program_test.add_sysvar_account(
        sysvar::clock::id(),
        &Clock {
            slot: 10,
            epoch_start_timestamp: 0,
            epoch: 0,
            leader_schedule_epoch: DEFAULT_SLOTS_PER_EPOCH,
            unix_timestamp: 100,
        },
    );

    program_test.start().await
}
