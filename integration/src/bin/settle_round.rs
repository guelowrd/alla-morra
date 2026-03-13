//! House operator: settle a Morra round by consuming both players' bet notes.
//!
//! Reconstructs both notes from game parameters + serial numbers provided by the players,
//! then submits a single settlement transaction from the house account.
//!
//! Usage:
//!   cargo run --bin settle_round -- \
//!     --round-id 42 \
//!     --house-id  mtst1<house-id> \
//!     --faucet-id mtst1<faucet-id> \
//!     --player1-id mtst1<p1-id> --p1-h 2 --p1-g 3 --p1-serial <64-hex> \
//!     --player2-id mtst1<p2-id> --p2-h 1 --p2-g 4 --p2-serial <64-hex> \
//!     [--bet-value 1000000] \
//!     [--expiry-block 999999] \
//!     [--data-dir ..]

use std::{path::PathBuf, sync::Arc};

use anyhow::{Context, Result};
use clap::Parser;
use integration::helpers::{
    build_project_in_dir, reconstruct_bet_note, setup_client_at, wait_for_new_block, ClientSetup,
};
use miden_client::{account::AccountId, transaction::TransactionRequestBuilder};

#[derive(Parser, Debug)]
#[command(about = "Settle a Morra round as the house operator")]
struct Args {
    /// Unique round identifier (must match what both players used in publish_bet)
    #[arg(long)]
    round_id: u64,

    /// House account ID (bech32)
    #[arg(long)]
    house_id: String,

    /// Faucet account ID (bech32, same one players used)
    #[arg(long)]
    faucet_id: String,

    /// Player 1's account ID (bech32)
    #[arg(long)]
    player1_id: String,

    /// Player 1's fingers shown
    #[arg(long)]
    p1_h: u64,

    /// Player 1's guess
    #[arg(long)]
    p1_g: u64,

    /// Player 1's note serial number (64 hex chars, printed by publish_bet)
    #[arg(long)]
    p1_serial: String,

    /// Player 2's account ID (bech32)
    #[arg(long)]
    player2_id: String,

    /// Player 2's fingers shown
    #[arg(long)]
    p2_h: u64,

    /// Player 2's guess
    #[arg(long)]
    p2_g: u64,

    /// Player 2's note serial number (64 hex chars, printed by publish_bet)
    #[arg(long)]
    p2_serial: String,

    /// Bet amount in base units (must match what players used)
    #[arg(long, default_value_t = 1_000_000)]
    bet_value: u64,

    /// Expiry block (must match what players used)
    #[arg(long, default_value_t = 999_999)]
    expiry_block: u64,

    /// Directory containing house keystore and store.sqlite3
    #[arg(long, default_value = "..")]
    data_dir: PathBuf,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    let (_, house_id) = AccountId::from_bech32(&args.house_id).context("invalid --house-id")?;
    let (_, faucet_id) = AccountId::from_bech32(&args.faucet_id).context("invalid --faucet-id")?;
    let (_, p1_id) = AccountId::from_bech32(&args.player1_id).context("invalid --player1-id")?;
    let (_, p2_id) = AccountId::from_bech32(&args.player2_id).context("invalid --player2-id")?;

    println!("╔══════════════════════════════════════╗");
    println!("║  Morra — Settle Round                ║");
    println!("╚══════════════════════════════════════╝\n");
    println!("  Round: {}", args.round_id);
    println!("  P1: h={}, g={}", args.p1_h, args.p1_g);
    println!("  P2: h={}, g={}", args.p2_h, args.p2_g);
    let total = args.p1_h + args.p2_h;
    println!("  Total fingers: {total}");
    let winner_payout = args.bet_value * 2 - args.bet_value * 2 / 100;
    let outcome = if args.p1_g == total && args.p2_g != total {
        format!("P1 WINS — payout {winner_payout} base units")
    } else if args.p2_g == total && args.p1_g != total {
        format!("P2 WINS — payout {winner_payout} base units")
    } else {
        format!("DRAW — {} base units each", winner_payout / 2)
    };
    println!("  Outcome: {outcome}\n");

    let ClientSetup { mut client, .. } = setup_client_at(&args.data_dir).await?;
    let sync = client.sync_state().await?;
    println!("Connected. Block: {}\n", sync.block_num);

    // Build bet-note package (needed to reconstruct the MAST forest / note script)
    println!("Building bet-note contract...");
    let note_package = Arc::new(
        build_project_in_dir(std::path::Path::new("./contracts/bet-note"), true)
            .context("build bet-note")?,
    );
    println!("  Done.\n");

    // Reconstruct both notes from game params + serial nums shared by players
    println!("Reconstructing bet notes...");
    let note1 = reconstruct_bet_note(
        &note_package,
        1,
        args.p1_h,
        args.p1_g,
        p1_id,
        &args.p1_serial,
        args.round_id,
        p1_id,
        p2_id,
        house_id,
        faucet_id,
        args.bet_value,
        args.expiry_block,
    )
    .context("reconstruct P1 note")?;
    println!("  P1 note ID: {}", note1.id().to_hex());

    let note2 = reconstruct_bet_note(
        &note_package,
        2,
        args.p2_h,
        args.p2_g,
        p2_id,
        &args.p2_serial,
        args.round_id,
        p1_id,
        p2_id,
        house_id,
        faucet_id,
        args.bet_value,
        args.expiry_block,
    )
    .context("reconstruct P2 note")?;
    println!("  P2 note ID: {}\n", note2.id().to_hex());

    // Wait for notes to be committed on-chain before submitting the settlement
    println!("Waiting for bet notes to be committed on-chain...");
    let current_block = wait_for_new_block(&mut client, sync.block_num, "bet notes").await?;

    // Submit settlement transaction — house consumes both notes in one tx
    println!("Submitting settlement transaction...");
    let req = TransactionRequestBuilder::new()
        .input_notes([(note1, None), (note2, None)])
        .build()
        .context("build settlement request")?;
    let tx = client
        .submit_new_transaction(house_id, req)
        .await
        .context("submit settlement transaction")?;

    wait_for_new_block(&mut client, current_block, "settlement confirmed").await?;

    println!("\n╔══════════════════════════════════════════════════════════╗");
    println!("║  Settlement complete!                                    ║");
    println!("╠══════════════════════════════════════════════════════════╣");
    println!("║  Tx:      {}", tx.to_hex());
    println!("║  Outcome: {outcome}");
    println!("╚══════════════════════════════════════════════════════════╝");

    Ok(())
}
