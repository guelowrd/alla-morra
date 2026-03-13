//! One-time player (or house) setup: create account, fund with faucet tokens.
//!
//! Usage (player):
//!   cargo run --bin setup_player -- [--data-dir ./my-data]
//!
//! Usage (house):
//!   cargo run --bin setup_player -- --role house [--data-dir ./house-data]
//!
//! Prints the account ID and faucet ID. Save them — they're needed for publish_bet / settle_round.

use std::{path::PathBuf, sync::Arc};

use anyhow::{Context, Result};
use clap::Parser;
use integration::helpers::{
    build_project_in_dir, create_account_from_package, create_basic_wallet_account,
    faucet_meta, request_faucet_tokens, setup_client_at, wait_for_consumable_note,
    wait_for_new_block, AccountCreationConfig, ClientSetup,
};
use miden_client::{
    account::{NetworkId, StorageMap, StorageSlot, StorageSlotName},
    note::Note,
    transaction::TransactionRequestBuilder,
};

#[derive(Parser, Debug)]
#[command(about = "Set up a Morra player or house account on Miden testnet")]
struct Args {
    /// "player" (default) or "house"
    #[arg(long, default_value = "player")]
    role: String,

    /// Directory for keystore and SQLite store (default: ..)
    #[arg(long, default_value = "..")]
    data_dir: PathBuf,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    match args.role.as_str() {
        "player" => setup_player(args.data_dir).await,
        "house" => setup_house(args.data_dir).await,
        other => anyhow::bail!("Unknown role '{}' — use 'player' or 'house'", other),
    }
}

async fn setup_player(data_dir: PathBuf) -> Result<()> {
    println!("╔══════════════════════════════════════╗");
    println!("║  Morra — Player Setup                ║");
    println!("╚══════════════════════════════════════╝\n");

    let http = reqwest::Client::new();
    let ClientSetup { mut client, keystore } = setup_client_at(&data_dir).await?;
    let sync = client.sync_state().await?;
    println!("Connected. Block: {}\n", sync.block_num);

    // Create wallet account
    println!("[1/4] Creating wallet account...");
    let player =
        create_basic_wallet_account(&mut client, keystore.clone(), Default::default()).await?;
    let player_bech32 = player.id().to_bech32(NetworkId::Testnet);
    println!("  Account: {player_bech32}\n");

    // Get faucet metadata
    println!("[2/4] Fetching faucet metadata...");
    let meta = faucet_meta(&http).await?;
    println!("  Faucet: {} ({} base units)\n", meta.id, meta.base_amount);

    // Request faucet tokens
    println!("[3/4] Requesting faucet tokens...");
    request_faucet_tokens(&http, &player_bech32, meta.base_amount, &meta).await?;
    println!();

    // Wait for faucet note and consume it
    println!("[4/4] Consuming faucet note...");
    wait_for_consumable_note(&mut client, player.id(), "faucet").await?;

    let consumable = client.get_consumable_notes(Some(player.id())).await?;
    let (record, _) = consumable
        .into_iter()
        .next()
        .context("no consumable faucet note found")?;
    let faucet_note: Note = record.try_into().context("convert faucet record to Note")?;
    let req = TransactionRequestBuilder::new()
        .input_notes([(faucet_note, None)])
        .build()
        .context("build consume request")?;
    let tx = client
        .submit_new_transaction(player.id(), req)
        .await
        .context("consume faucet note")?;
    println!("  Tx: {}", tx.to_hex());
    wait_for_new_block(&mut client, sync.block_num, "token confirmation").await?;

    println!("\n╔══════════════════════════════════════════════════════════╗");
    println!("║  Player setup complete!                                  ║");
    println!("╠══════════════════════════════════════════════════════════╣");
    println!("║  Account ID: {player_bech32}");
    println!("║  Faucet ID:  {}", meta.id);
    println!("║  Balance:    {} base units", meta.base_amount);
    println!("╠══════════════════════════════════════════════════════════╣");
    println!("║  Pass these to publish_bet:                              ║");
    println!("║    --my-id {player_bech32}");
    println!("║    --faucet-id {}", meta.id);
    println!("╚══════════════════════════════════════════════════════════╝");

    Ok(())
}

async fn setup_house(data_dir: PathBuf) -> Result<()> {
    println!("╔══════════════════════════════════════╗");
    println!("║  Morra — House Setup                 ║");
    println!("╚══════════════════════════════════════╝\n");

    let ClientSetup { mut client, .. } = setup_client_at(&data_dir).await?;
    client.sync_state().await?;

    println!("[1/2] Building house-account contract...");
    let house_package = Arc::new(
        build_project_in_dir(std::path::Path::new("./contracts/house-account"), true)
            .context("build house-account")?,
    );
    println!("  Done.\n");

    println!("[2/2] Creating house account on testnet...");
    let bets_slot =
        StorageSlotName::new("miden::component::miden_house_account::bets").unwrap();
    let house_cfg = AccountCreationConfig {
        storage_slots: vec![StorageSlot::with_map(bets_slot, StorageMap::default())],
        ..Default::default()
    };
    let house = create_account_from_package(&mut client, house_package, house_cfg).await?;
    let house_bech32 = house.id().to_bech32(NetworkId::Testnet);

    println!("\n╔══════════════════════════════════════════════════════════╗");
    println!("║  House setup complete!                                   ║");
    println!("╠══════════════════════════════════════════════════════════╣");
    println!("║  House ID: {house_bech32}");
    println!("╠══════════════════════════════════════════════════════════╣");
    println!("║  Share this house ID with both players.                  ║");
    println!("║  Pass it to settle_round: --house-id {house_bech32}");
    println!("╚══════════════════════════════════════════════════════════╝");

    Ok(())
}
