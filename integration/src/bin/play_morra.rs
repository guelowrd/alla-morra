//! Morra on Miden — testnet end-to-end demo
//!
//! Flow:
//!   1. Create house, player1, player2 accounts on testnet
//!   2. Request tokens from the public Miden faucet for each player (PoW)
//!   3. Players consume their P2ID faucet notes → tokens in vault
//!   4. Each player publishes their bet note (custom script + asset)
//!   5. House settles the round by consuming both bet notes
//!   6. Verify payout output note

use std::{path::Path, sync::Arc, time::Duration};

use anyhow::{Context, Result};
use integration::helpers::{
    build_project_in_dir, create_account_from_package, create_basic_wallet_account,
    create_note_from_package, setup_client, AccountCreationConfig, ClientSetup, NoteCreationConfig,
};
use miden_client::{
    account::{Account, AccountId, NetworkId, StorageMap, StorageSlot, StorageSlotName},
    asset::{Asset, FungibleAsset},
    block::BlockNumber,
    note::{NoteAssets, NoteTag, NoteType},
    store::NoteFilter,
    transaction::{OutputNote, TransactionRequestBuilder},
    Felt,
};
use sha2::{Digest as Sha2Digest, Sha256};

// ── Game constants ────────────────────────────────────────────────────────────

const FAUCET_API: &str = "https://faucet-api-testnet-miden.eu-central-8.gateway.fm";

/// 1 display token (faucet has 6 decimals, so 1_000_000 base units = 1 token).
/// Must be divisible by 50 for the fee math to be exact.
const BET_VALUE: u64 = 1_000_000;

const ROUND_ID: u64 = 1;

// Player 1 shows 1 finger and guesses the total will be 2.
// Player 2 shows 1 finger and guesses the total will be 0.
// Total = 2, P1's guess matches → P1 wins.
const H1: u64 = 1;
const G1: u64 = 2;
const H2: u64 = 1;
const G2: u64 = 0;

/// Far-future block number so the note never expires in this test.
const EXPIRY_BLOCK: u64 = 999_999;

// ── Faucet helpers ────────────────────────────────────────────────────────────

#[derive(serde::Deserialize)]
struct FaucetMeta {
    id: String,
    base_amount: u64,
    pow_load_difficulty: u64,
}

#[derive(serde::Deserialize)]
struct PowChallenge {
    challenge: String,
    target: u64,
}

async fn faucet_meta(http: &reqwest::Client) -> Result<FaucetMeta> {
    Ok(http
        .get(format!("{FAUCET_API}/get_metadata"))
        .send()
        .await?
        .json()
        .await?)
}

/// CPU-bound PoW: find a nonce such that SHA-256(challenge || nonce_be) ≤ target.
fn solve_pow(challenge_hex: &str, target: u64) -> u64 {
    let challenge = hex::decode(challenge_hex).expect("faucet challenge is valid hex");
    let mut nonce = 0u64;
    loop {
        let mut data = challenge.clone();
        data.extend_from_slice(&nonce.to_be_bytes());
        let hash = Sha256::digest(&data);
        let val = u64::from_be_bytes(hash[..8].try_into().unwrap());
        if val <= target {
            return nonce;
        }
        nonce += 1;
    }
}

/// Requests `amount` base-unit tokens from the faucet for `account_id` (bech32).
/// Solves the PoW challenge automatically. Returns the on-chain note_id.
async fn request_faucet_tokens(
    http: &reqwest::Client,
    account_id_bech32: &str,
    amount: u64,
    meta: &FaucetMeta,
) -> Result<String> {
    // 1. Get PoW challenge
    let pow: PowChallenge = http
        .get(format!("{FAUCET_API}/pow"))
        .query(&[("account_id", account_id_bech32), ("amount", &amount.to_string())])
        .send()
        .await?
        .json()
        .await?;
    println!("    PoW target: {} (difficulty ~{})", pow.target, meta.pow_load_difficulty);

    // 2. Solve on the current thread (fast — expected ~131 k iterations at 1× difficulty)
    let nonce = solve_pow(&pow.challenge, pow.target);
    println!("    Solved with nonce: {nonce}");

    // 3. Claim tokens (public note so it appears in sync without manual import)
    let resp: serde_json::Value = http
        .get(format!("{FAUCET_API}/get_tokens"))
        .query(&[
            ("account_id", account_id_bech32),
            ("is_private_note", "false"),
            ("asset_amount", &amount.to_string()),
            ("challenge", &pow.challenge),
            ("nonce", &nonce.to_string()),
        ])
        .send()
        .await?
        .json()
        .await?;

    println!("    Faucet response: {resp}");
    let note_id = resp["note_id"]
        .as_str()
        .context("no note_id in faucet response")?
        .to_string();
    Ok(note_id)
}

// ── Sync helpers ──────────────────────────────────────────────────────────────

type MidenClient = miden_client::Client<miden_client::keystore::FilesystemKeyStore>;

/// Syncs repeatedly until at least one new block has been committed.
async fn wait_for_new_block(
    client: &mut MidenClient,
    from_block: BlockNumber,
    label: &str,
) -> Result<BlockNumber> {
    println!("  [{label}] waiting for next block...");
    loop {
        tokio::time::sleep(Duration::from_secs(8)).await;
        let summary = client.sync_state().await?;
        println!("  [{label}] block {}", summary.block_num);
        if summary.block_num > from_block {
            return Ok(summary.block_num);
        }
    }
}

/// Syncs repeatedly until `get_consumable_notes` returns at least one note for
/// `account_id`. Returns on the first hit.
async fn wait_for_consumable_note(
    client: &mut MidenClient,
    account_id: AccountId,
    label: &str,
) -> Result<()> {
    println!("  [{label}] waiting for consumable note...");
    loop {
        tokio::time::sleep(Duration::from_secs(8)).await;
        client.sync_state().await?;
        let consumable = client.get_consumable_notes(Some(account_id)).await?;
        if !consumable.is_empty() {
            println!("  [{label}] {} consumable note(s) found", consumable.len());
            return Ok(());
        }
        println!("  [{label}] none yet, retrying...");
    }
}

// ── Note input builder ────────────────────────────────────────────────────────

fn id_felts(id: AccountId) -> (Felt, Felt) {
    (id.suffix(), id.prefix().into())
}

fn make_note_inputs(
    round_id: u64,
    player_num: u64,
    h: u64,
    g: u64,
    p1: AccountId,
    p2: AccountId,
    house: AccountId,
    bet_value: u64,
    expiry_block: u64,
) -> Vec<Felt> {
    let (p1_s, p1_p) = id_felts(p1);
    let (p2_s, p2_p) = id_felts(p2);
    let (hs_s, hs_p) = id_felts(house);
    vec![
        Felt::new(round_id),
        Felt::new(player_num),
        Felt::new(h),
        Felt::new(g),
        p1_s,
        p1_p,
        p2_s,
        p2_p,
        hs_s,
        hs_p,
        Felt::new(bet_value),
        Felt::new(expiry_block),
    ]
}

// ── Main ─────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    println!("╔══════════════════════════════════════╗");
    println!("║  Morra on Miden — testnet demo       ║");
    println!("╚══════════════════════════════════════╝\n");

    let http = reqwest::Client::new();
    let ClientSetup { mut client, keystore } = setup_client().await?;

    let sync = client.sync_state().await?;
    println!("Connected. Latest block: {}\n", sync.block_num);
    let mut current_block = sync.block_num;

    // ── Step 1: Build contracts ───────────────────────────────────────────────
    println!("[1/8] Building contracts...");
    let house_package = Arc::new(
        build_project_in_dir(Path::new("./contracts/house-account"), true)
            .context("build house-account")?,
    );
    let note_package = Arc::new(
        build_project_in_dir(Path::new("./contracts/bet-note"), true)
            .context("build bet-note")?,
    );
    println!("  Done.\n");

    // ── Step 2: Create accounts ───────────────────────────────────────────────
    println!("[2/8] Creating accounts on testnet...");

    let bets_slot =
        StorageSlotName::new("miden::component::miden_house_account::bets").unwrap();
    let house_cfg = AccountCreationConfig {
        storage_slots: vec![StorageSlot::with_map(bets_slot, StorageMap::default())],
        ..Default::default()
    };
    let house: Account =
        create_account_from_package(&mut client, house_package.clone(), house_cfg).await?;
    println!("  House:   {}", house.id().to_bech32(NetworkId::Testnet));

    let player1 =
        create_basic_wallet_account(&mut client, keystore.clone(), Default::default()).await?;
    println!("  Player1: {}", player1.id().to_bech32(NetworkId::Testnet));

    let player2 =
        create_basic_wallet_account(&mut client, keystore.clone(), Default::default()).await?;
    println!("  Player2: {}", player2.id().to_bech32(NetworkId::Testnet));
    println!();

    // ── Step 3: Get faucet metadata & parse faucet account ID ────────────────
    println!("[3/8] Fetching faucet metadata...");
    let meta = faucet_meta(&http).await?;
    println!("  Faucet:  {}", meta.id);
    println!("  Token:   {} base units per request\n", meta.base_amount);

    let (_, faucet_account_id) =
        AccountId::from_bech32(&meta.id).context("parse faucet account id")?;

    // ── Step 4: Request tokens for both players ───────────────────────────────
    println!("[4/8] Requesting faucet tokens...");

    println!("  Player1:");
    let _nid1 = request_faucet_tokens(
        &http,
        &player1.id().to_bech32(NetworkId::Testnet),
        meta.base_amount,
        &meta,
    )
    .await?;

    println!("  Player2:");
    let _nid2 = request_faucet_tokens(
        &http,
        &player2.id().to_bech32(NetworkId::Testnet),
        meta.base_amount,
        &meta,
    )
    .await?;
    println!();

    // ── Step 5: Wait for faucet notes to land on-chain ────────────────────────
    println!("[5/8] Waiting for faucet notes to be committed...");
    wait_for_consumable_note(&mut client, player1.id(), "player1 faucet note").await?;
    wait_for_consumable_note(&mut client, player2.id(), "player2 faucet note").await?;
    println!();

    // ── Step 6: Players consume their faucet notes → tokens in vault ──────────
    println!("[6/8] Players consuming faucet notes...");

    // Player 1
    let p1_consumable = client.get_consumable_notes(Some(player1.id())).await?;
    let (p1_faucet_record, _) = p1_consumable.into_iter().next().context("no faucet note for player1")?;
    let p1_faucet_note: miden_client::note::Note = p1_faucet_record.try_into()
        .context("convert player1 faucet record to Note")?;
    let p1_consume_req = TransactionRequestBuilder::new()
        .input_notes([(p1_faucet_note, None)])
        .build()
        .context("build player1 faucet consume request")?;
    let p1_consume_tx = client
        .submit_new_transaction(player1.id(), p1_consume_req)
        .await
        .context("player1 consume faucet note")?;
    println!("  Player1 consume tx: {}", p1_consume_tx.to_hex());

    // Player 2
    let p2_consumable = client.get_consumable_notes(Some(player2.id())).await?;
    let (p2_faucet_record, _) = p2_consumable.into_iter().next().context("no faucet note for player2")?;
    let p2_faucet_note: miden_client::note::Note = p2_faucet_record.try_into()
        .context("convert player2 faucet record to Note")?;
    let p2_consume_req = TransactionRequestBuilder::new()
        .input_notes([(p2_faucet_note, None)])
        .build()
        .context("build player2 faucet consume request")?;
    let p2_consume_tx = client
        .submit_new_transaction(player2.id(), p2_consume_req)
        .await
        .context("player2 consume faucet note")?;
    println!("  Player2 consume tx: {}", p2_consume_tx.to_hex());

    current_block = wait_for_new_block(&mut client, current_block, "faucet consumption").await?;
    println!();

    // ── Step 7: Players publish their bet notes ───────────────────────────────
    println!("[7/8] Publishing bet notes...");

    let faucet_asset = |amount: u64| -> Result<NoteAssets> {
        let asset = Asset::Fungible(FungibleAsset::new(faucet_account_id, amount)?);
        Ok(NoteAssets::new(vec![asset])?)
    };

    let bet_note_1 = create_note_from_package(
        &mut client,
        note_package.clone(),
        player1.id(),
        NoteCreationConfig {
            note_type: NoteType::Public,
            tag: NoteTag::new(0),
            assets: faucet_asset(BET_VALUE)?,
            inputs: make_note_inputs(
                ROUND_ID, 1, H1, G1,
                player1.id(), player2.id(), house.id(),
                BET_VALUE, EXPIRY_BLOCK,
            ),
        },
    )
    .context("create bet_note_1")?;
    println!("  bet_note_1 id: {}", bet_note_1.id().to_hex());

    let bet_note_2 = create_note_from_package(
        &mut client,
        note_package.clone(),
        player2.id(),
        NoteCreationConfig {
            note_type: NoteType::Public,
            tag: NoteTag::new(0),
            assets: faucet_asset(BET_VALUE)?,
            inputs: make_note_inputs(
                ROUND_ID, 2, H2, G2,
                player1.id(), player2.id(), house.id(),
                BET_VALUE, EXPIRY_BLOCK,
            ),
        },
    )
    .context("create bet_note_2")?;
    println!("  bet_note_2 id: {}", bet_note_2.id().to_hex());

    // Player 1 publishes bet note (deducts BET_VALUE from vault → output note)
    let p1_publish_req = TransactionRequestBuilder::new()
        .own_output_notes(vec![OutputNote::Full(bet_note_1.clone())])
        .build()
        .context("build player1 publish request")?;
    let p1_publish_tx = client
        .submit_new_transaction(player1.id(), p1_publish_req)
        .await
        .context("player1 publish bet note")?;
    println!("  Player1 publish tx: {}", p1_publish_tx.to_hex());

    // Player 2 publishes bet note
    let p2_publish_req = TransactionRequestBuilder::new()
        .own_output_notes(vec![OutputNote::Full(bet_note_2.clone())])
        .build()
        .context("build player2 publish request")?;
    let p2_publish_tx = client
        .submit_new_transaction(player2.id(), p2_publish_req)
        .await
        .context("player2 publish bet note")?;
    println!("  Player2 publish tx: {}", p2_publish_tx.to_hex());

    current_block = wait_for_new_block(&mut client, current_block, "bet notes committed").await?;
    println!();

    // ── Step 8: House settles — consumes both bet notes in one transaction ────
    println!("[8/8] House settling the round...");

    let settle_req = TransactionRequestBuilder::new()
        .input_notes([(bet_note_1, None), (bet_note_2, None)])
        .build()
        .context("build house settle request")?;
    let settle_tx = client
        .submit_new_transaction(house.id(), settle_req)
        .await
        .context("house settle transaction")?;
    println!("  Settlement tx: {}", settle_tx.to_hex());

    let _ = wait_for_new_block(&mut client, current_block, "settlement").await?;

    // ── Results ───────────────────────────────────────────────────────────────
    println!();
    println!("╔══════════════════════════════════════╗");
    println!("║  GAME RESULT                         ║");
    println!("╠══════════════════════════════════════╣");
    let winner_payout = BET_VALUE * 2 - (BET_VALUE * 2 / 100);
    println!("║  P1 fingers: {H1}, guess: {G1}               ║");
    println!("║  P2 fingers: {H2}, guess: {G2}               ║");
    println!("║  Total shown: {}                       ║", H1 + H2);
    if G1 == H1 + H2 && G2 != H1 + H2 {
        println!("║  Winner: PLAYER 1                    ║");
        println!("║  Payout: {winner_payout} base units          ║");
    } else if G2 == H1 + H2 && G1 != H1 + H2 {
        println!("║  Winner: PLAYER 2                    ║");
        println!("║  Payout: {winner_payout} base units          ║");
    } else {
        let draw_payout = winner_payout / 2;
        println!("║  Result: DRAW                        ║");
        println!("║  Each gets: {draw_payout} base units        ║");
    }
    println!("╠══════════════════════════════════════╣");
    println!("║  Settlement tx:                      ║");
    println!("║  {}", &settle_tx.to_hex()[..36]);
    println!("╚══════════════════════════════════════╝");

    Ok(())
}
