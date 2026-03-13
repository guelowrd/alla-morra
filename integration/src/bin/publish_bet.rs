//! Publish a player's bet note to the Miden testnet.
//!
//! Run after setup_player. Deducts bet_value from your vault and publishes a
//! public note targeting the house account.
//!
//! Usage:
//!   cargo run --bin publish_bet -- \
//!     --round-id 42 \
//!     --player-num 1 \
//!     --h 2 --g 3 \
//!     --my-id     mtst1<your-account-id> \
//!     --player1-id mtst1<p1-id> \
//!     --player2-id mtst1<p2-id> \
//!     --house-id  mtst1<house-id> \
//!     --faucet-id mtst1<faucet-id> \
//!     [--bet-value 1000000] \
//!     [--expiry-block 999999] \
//!     [--data-dir ..]
//!
//! Prints the note serial number (64 hex chars) — share it with the house operator.

use std::{path::PathBuf, sync::Arc};

use anyhow::{Context, Result};
use clap::Parser;
use integration::helpers::{
    build_project_in_dir, create_note_from_package, encode_word, make_note_inputs,
    setup_client_at, wait_for_new_block, ClientSetup, NoteCreationConfig,
};
use miden_client::{
    account::AccountId,
    asset::{Asset, FungibleAsset},
    note::{NoteAssets, NoteTag, NoteType},
    transaction::{OutputNote, TransactionRequestBuilder},
};

#[derive(Parser, Debug)]
#[command(about = "Publish a Morra bet note to Miden testnet")]
struct Args {
    /// Unique round identifier shared by both players and the house
    #[arg(long)]
    round_id: u64,

    /// Your player number: 1 or 2
    #[arg(long)]
    player_num: u64,

    /// Fingers you show: 0–3
    #[arg(long)]
    h: u64,

    /// Your guess for the total fingers: 0–6
    #[arg(long)]
    g: u64,

    /// Your own account ID (bech32, from setup_player)
    #[arg(long)]
    my_id: String,

    /// Player 1's account ID (bech32)
    #[arg(long)]
    player1_id: String,

    /// Player 2's account ID (bech32)
    #[arg(long)]
    player2_id: String,

    /// House account ID (bech32, from setup_player --role house)
    #[arg(long)]
    house_id: String,

    /// Faucet account ID (bech32, printed by setup_player)
    #[arg(long)]
    faucet_id: String,

    /// Bet amount in base units (must be divisible by 50)
    #[arg(long, default_value_t = 1_000_000)]
    bet_value: u64,

    /// Block number after which the note can be recalled (house-mediated refund)
    #[arg(long, default_value_t = 999_999)]
    expiry_block: u64,

    /// Directory containing your keystore and store.sqlite3
    #[arg(long, default_value = "..")]
    data_dir: PathBuf,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Validate inputs early
    anyhow::ensure!(args.player_num == 1 || args.player_num == 2, "--player-num must be 1 or 2");
    anyhow::ensure!(args.h <= 3, "--h must be 0–3");
    anyhow::ensure!(args.g <= 6, "--g must be 0–6");
    anyhow::ensure!(args.bet_value % 50 == 0, "--bet-value must be divisible by 50");

    let (_, my_id) = AccountId::from_bech32(&args.my_id).context("invalid --my-id")?;
    let (_, p1_id) = AccountId::from_bech32(&args.player1_id).context("invalid --player1-id")?;
    let (_, p2_id) = AccountId::from_bech32(&args.player2_id).context("invalid --player2-id")?;
    let (_, house_id) = AccountId::from_bech32(&args.house_id).context("invalid --house-id")?;
    let (_, faucet_id) = AccountId::from_bech32(&args.faucet_id).context("invalid --faucet-id")?;

    println!("╔══════════════════════════════════════╗");
    println!("║  Morra — Publish Bet                 ║");
    println!("╚══════════════════════════════════════╝\n");
    println!("  Round:  {}", args.round_id);
    println!("  Player: {} (h={}, g={})", args.player_num, args.h, args.g);
    println!("  Bet:    {} base units\n", args.bet_value);

    let ClientSetup { mut client, .. } = setup_client_at(&args.data_dir).await?;
    let sync = client.sync_state().await?;
    println!("Connected. Block: {}\n", sync.block_num);

    // Build the bet-note contract
    println!("Building bet-note contract...");
    let note_package = Arc::new(
        build_project_in_dir(std::path::Path::new("./contracts/bet-note"), true)
            .context("build bet-note")?,
    );
    println!("  Done.\n");

    // Create the note object (random serial num from client RNG)
    let asset = Asset::Fungible(FungibleAsset::new(faucet_id, args.bet_value)?);
    let bet_note = create_note_from_package(
        &mut client,
        note_package,
        my_id,
        NoteCreationConfig {
            note_type: NoteType::Public,
            tag: NoteTag::new(0),
            assets: NoteAssets::new(vec![asset]).context("create note assets")?,
            inputs: make_note_inputs(
                args.round_id,
                args.player_num,
                args.h,
                args.g,
                p1_id,
                p2_id,
                house_id,
                args.bet_value,
                args.expiry_block,
            ),
        },
    )
    .context("create bet note")?;

    // Encode serial_num — the house needs this to reconstruct and consume the note
    let serial_hex = encode_word(bet_note.recipient().serial_num());
    let note_id_hex = bet_note.id().to_hex();

    // Publish: submit a tx that moves bet_value from player vault → output note
    println!("Publishing bet note...");
    let req = TransactionRequestBuilder::new()
        .own_output_notes(vec![OutputNote::Full(bet_note)])
        .build()
        .context("build publish request")?;
    let tx = client
        .submit_new_transaction(my_id, req)
        .await
        .context("submit publish transaction")?;
    println!("  Tx: {}", tx.to_hex());

    wait_for_new_block(&mut client, sync.block_num, "bet note committed").await?;

    println!("\n╔══════════════════════════════════════════════════════════╗");
    println!("║  Bet published!                                          ║");
    println!("╠══════════════════════════════════════════════════════════╣");
    println!("║  Note ID:     {note_id_hex}");
    println!("║  Serial (hex): {serial_hex}");
    println!("╠══════════════════════════════════════════════════════════╣");
    println!("║  Send to house operator:                                 ║");
    println!("║    --p{}-serial {serial_hex}", args.player_num);
    println!("╚══════════════════════════════════════════════════════════╝");

    Ok(())
}
