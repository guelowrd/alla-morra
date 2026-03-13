use integration::helpers::{
    build_project_in_dir, create_testing_account_from_package, create_testing_note_from_package,
    AccountCreationConfig, NoteCreationConfig,
};

use miden_client::{
    account::{AccountId, StorageMap, StorageSlot, StorageSlotName},
    asset::{Asset, FungibleAsset},
    note::{NoteAssets, NoteType},
    transaction::OutputNote,
    Felt, Word,
};
use miden_testing::{Auth, MockChain};
use std::{path::Path, sync::Arc};

// ── Helpers ────────────────────────────────────────────────────────────────────

/// Extracts the (suffix, prefix) Felt components from an AccountId.
///
/// In miden-objects 0.20, AccountId is split into prefix (high bits encoding
/// account type/storage mode) and suffix (low bits, random component).
/// These correspond to `active_account::get_id().suffix` and `.prefix` in contracts.
fn id_felts(id: AccountId) -> (Felt, Felt) {
    let suffix = id.suffix().as_felt();
    let prefix = id.prefix().as_felt();
    (suffix, prefix)
}

/// Builds note inputs for a Morra bet note (12 Felts).
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
        Felt::new(round_id),     // [0]  round_id
        Felt::new(player_num),   // [1]  player_num
        Felt::new(h),            // [2]  h (fingers shown)
        Felt::new(g),            // [3]  g (guess of total)
        p1_s,                    // [4]  player1_id_suffix
        p1_p,                    // [5]  player1_id_prefix
        p2_s,                    // [6]  player2_id_suffix
        p2_p,                    // [7]  player2_id_prefix
        hs_s,                    // [8]  house_id_suffix
        hs_p,                    // [9]  house_id_prefix
        Felt::new(bet_value),    // [10] bet_value
        Felt::new(expiry_block), // [11] expiry_block
    ]
}

/// Shared test setup: builds packages and creates accounts.
struct TestSetup {
    mock_chain: MockChain,
    house_package: Arc<miden_mast_package::Package>,
    note_package: Arc<miden_mast_package::Package>,
    player1: miden_client::account::Account,
    player2: miden_client::account::Account,
    house: miden_client::account::Account,
    faucet: miden_client::account::Account,
}

async fn setup() -> anyhow::Result<TestSetup> {
    let mut builder = MockChain::builder();

    let player1 = builder.add_existing_wallet(Auth::BasicAuth)?;
    let player2 = builder.add_existing_wallet(Auth::BasicAuth)?;

    // Build contracts (house-account first; bet-note depends on its generated WIT)
    let house_package = Arc::new(build_project_in_dir(
        Path::new("../contracts/house-account"),
        true,
    )?);
    let note_package = Arc::new(build_project_in_dir(
        Path::new("../contracts/bet-note"),
        true,
    )?);

    // Create house account with empty bets storage map
    let bets_slot =
        StorageSlotName::new("miden::component::miden_house_account::bets").unwrap();
    let storage_slots = vec![StorageSlot::with_map(
        bets_slot,
        StorageMap::default(),
    )];
    let house_cfg = AccountCreationConfig {
        storage_slots,
        ..Default::default()
    };
    let house =
        create_testing_account_from_package(house_package.clone(), house_cfg).await?;

    // Create a fungible faucet for the bet asset
    let faucet = builder.add_existing_faucet(Auth::NoAuth, "MORRA", 1_000_000_000_000)?;

    builder.add_account(house.clone())?;

    let mock_chain = builder.build()?;

    Ok(TestSetup {
        mock_chain,
        house_package,
        note_package,
        player1,
        player2,
        house,
        faucet,
    })
}

/// Creates both bet notes for a round and adds them to the mock chain.
fn make_bet_notes(
    setup: &mut TestSetup,
    round_id: u64,
    h1: u64,
    g1: u64,
    h2: u64,
    g2: u64,
    bet_value: u64,
    expiry_block: u64,
) -> anyhow::Result<(
    miden_client::note::Note,
    miden_client::note::Note,
)> {
    let faucet_id = setup.faucet.id();
    let asset1 = Asset::Fungible(FungibleAsset::new(faucet_id, bet_value)?);
    let asset2 = Asset::Fungible(FungibleAsset::new(faucet_id, bet_value)?);
    let note_assets_1 = NoteAssets::new(vec![asset1])?;
    let note_assets_2 = NoteAssets::new(vec![asset2])?;

    let p1_id = setup.player1.id();
    let p2_id = setup.player2.id();
    let house_id = setup.house.id();

    let inputs1 = make_note_inputs(
        round_id, 1, h1, g1, p1_id, p2_id, house_id, bet_value, expiry_block,
    );
    let inputs2 = make_note_inputs(
        round_id, 2, h2, g2, p1_id, p2_id, house_id, bet_value, expiry_block,
    );

    let bet_note_1 = create_testing_note_from_package(
        setup.note_package.clone(),
        p1_id,
        NoteCreationConfig {
            note_type: NoteType::Private,
            assets: note_assets_1,
            inputs: inputs1,
            ..Default::default()
        },
    )?;
    let bet_note_2 = create_testing_note_from_package(
        setup.note_package.clone(),
        p2_id,
        NoteCreationConfig {
            note_type: NoteType::Private,
            assets: note_assets_2,
            inputs: inputs2,
            ..Default::default()
        },
    )?;

    setup.mock_chain.add_output_note(OutputNote::Full(bet_note_1.clone()));
    setup.mock_chain.add_output_note(OutputNote::Full(bet_note_2.clone()));

    Ok((bet_note_1, bet_note_2))
}

// ── Happy-path tests ───────────────────────────────────────────────────────────

const BET_VALUE: u64 = 1_000_000; // 1 MIDEN (divisible by 50)
const EXPIRY: u64 = 9999;

fn expected_winner_payout(bv: u64) -> u64 {
    let pot = bv * 2;
    pot - pot / 100
}

fn expected_draw_payout(bv: u64) -> u64 {
    expected_winner_payout(bv) / 2
}

/// h1=1, g1=2, h2=1, g2=0 → total=2 → P1 wins
#[tokio::test]
async fn p1_wins() -> anyhow::Result<()> {
    let mut s = setup().await?;
    let (n1, n2) = make_bet_notes(&mut s, 1, 1, 2, 1, 0, BET_VALUE, EXPIRY)?;

    let tx_context = s
        .mock_chain
        .build_tx_context(s.house.id(), &[n1.id(), n2.id()], &[])?
        .build()?;
    let executed = tx_context.execute().await?;

    let output_notes: Vec<_> = executed.output_notes().iter().collect();
    assert_eq!(output_notes.len(), 1, "expected 1 payout note for P1 win");

    // Winner payout: 2*BET_VALUE * 0.99
    let payout = expected_winner_payout(BET_VALUE);
    let note_amount = output_notes[0]
        .assets()
        .and_then(|a| a.iter().next())
        .map(|a| a.unwrap_fungible().amount())
        .unwrap_or(0);
    assert_eq!(note_amount, payout, "incorrect winner payout");

    println!("p1_wins passed — payout: {payout}");
    Ok(())
}

/// h1=1, g1=0, h2=1, g2=2 → total=2 → P2 wins
#[tokio::test]
async fn p2_wins() -> anyhow::Result<()> {
    let mut s = setup().await?;
    let (n1, n2) = make_bet_notes(&mut s, 2, 1, 0, 1, 2, BET_VALUE, EXPIRY)?;

    let tx_context = s
        .mock_chain
        .build_tx_context(s.house.id(), &[n1.id(), n2.id()], &[])?
        .build()?;
    let executed = tx_context.execute().await?;

    let output_notes: Vec<_> = executed.output_notes().iter().collect();
    assert_eq!(output_notes.len(), 1, "expected 1 payout note for P2 win");

    let payout = expected_winner_payout(BET_VALUE);
    let note_amount = output_notes[0]
        .assets()
        .and_then(|a| a.iter().next())
        .map(|a| a.unwrap_fungible().amount())
        .unwrap_or(0);
    assert_eq!(note_amount, payout, "incorrect winner payout");

    println!("p2_wins passed — payout: {payout}");
    Ok(())
}

/// h1=1, g1=2, h2=1, g2=2 → total=2 → both correct → draw
#[tokio::test]
async fn draw_both_correct() -> anyhow::Result<()> {
    let mut s = setup().await?;
    let (n1, n2) = make_bet_notes(&mut s, 3, 1, 2, 1, 2, BET_VALUE, EXPIRY)?;

    let tx_context = s
        .mock_chain
        .build_tx_context(s.house.id(), &[n1.id(), n2.id()], &[])?
        .build()?;
    let executed = tx_context.execute().await?;

    let output_notes: Vec<_> = executed.output_notes().iter().collect();
    assert_eq!(output_notes.len(), 2, "expected 2 payout notes for draw");

    let payout = expected_draw_payout(BET_VALUE);
    for note in &output_notes {
        let amount = note
            .assets()
            .and_then(|a| a.iter().next())
            .map(|a| a.unwrap_fungible().amount())
            .unwrap_or(0);
        assert_eq!(amount, payout, "incorrect draw payout");
    }

    println!("draw_both_correct passed — each payout: {payout}");
    Ok(())
}

/// h1=1, g1=0, h2=1, g2=1 → total=2 → both wrong → draw
#[tokio::test]
async fn draw_both_wrong() -> anyhow::Result<()> {
    let mut s = setup().await?;
    let (n1, n2) = make_bet_notes(&mut s, 4, 1, 0, 1, 1, BET_VALUE, EXPIRY)?;

    let tx_context = s
        .mock_chain
        .build_tx_context(s.house.id(), &[n1.id(), n2.id()], &[])?
        .build()?;
    let executed = tx_context.execute().await?;

    let output_notes: Vec<_> = executed.output_notes().iter().collect();
    assert_eq!(output_notes.len(), 2, "expected 2 payout notes for draw");

    let payout = expected_draw_payout(BET_VALUE);
    for note in &output_notes {
        let amount = note
            .assets()
            .and_then(|a| a.iter().next())
            .map(|a| a.unwrap_fungible().amount())
            .unwrap_or(0);
        assert_eq!(amount, payout, "incorrect draw payout");
    }

    println!("draw_both_wrong passed — each payout: {payout}");
    Ok(())
}

// ── Failure tests ──────────────────────────────────────────────────────────────

/// h=4 is invalid — tx must fail
#[tokio::test]
async fn invalid_h() -> anyhow::Result<()> {
    let mut s = setup().await?;
    let (n1, n2) = make_bet_notes(&mut s, 10, 4, 0, 1, 0, BET_VALUE, EXPIRY)?; // h1=4 invalid

    let tx_context = s
        .mock_chain
        .build_tx_context(s.house.id(), &[n1.id(), n2.id()], &[])?
        .build()?;
    let result = tx_context.execute().await;

    assert!(result.is_err(), "tx should fail for h=4");
    println!("invalid_h passed — tx correctly rejected");
    Ok(())
}

/// g=7 is invalid — tx must fail
#[tokio::test]
async fn invalid_g() -> anyhow::Result<()> {
    let mut s = setup().await?;
    let (n1, n2) = make_bet_notes(&mut s, 11, 1, 7, 1, 0, BET_VALUE, EXPIRY)?; // g1=7 invalid

    let tx_context = s
        .mock_chain
        .build_tx_context(s.house.id(), &[n1.id(), n2.id()], &[])?
        .build()?;
    let result = tx_context.execute().await;

    assert!(result.is_err(), "tx should fail for g=7");
    println!("invalid_g passed — tx correctly rejected");
    Ok(())
}

/// player_num=7 is invalid — tx must fail
#[tokio::test]
async fn invalid_player_num() -> anyhow::Result<()> {
    let mut s = setup().await?;

    // Manually craft a note with player_num=7
    let faucet_id = s.faucet.id();
    let asset = Asset::Fungible(FungibleAsset::new(faucet_id, BET_VALUE)?);
    let note_assets = NoteAssets::new(vec![asset])?;

    let (p1_s, p1_p) = id_felts(s.player1.id());
    let (p2_s, p2_p) = id_felts(s.player2.id());
    let (hs_s, hs_p) = id_felts(s.house.id());

    let bad_inputs = vec![
        Felt::new(12), Felt::new(7), // round_id=12, player_num=7 (invalid)
        Felt::new(1), Felt::new(2),
        p1_s, p1_p, p2_s, p2_p, hs_s, hs_p,
        Felt::new(BET_VALUE), Felt::new(EXPIRY),
    ];

    let bad_note = create_testing_note_from_package(
        s.note_package.clone(),
        s.player1.id(),
        NoteCreationConfig {
            note_type: NoteType::Private,
            assets: note_assets,
            inputs: bad_inputs,
            ..Default::default()
        },
    )?;
    s.mock_chain.add_output_note(OutputNote::Full(bad_note.clone()));

    let tx_context = s
        .mock_chain
        .build_tx_context(s.house.id(), &[bad_note.id()], &[])?
        .build()?;
    let result = tx_context.execute().await;

    assert!(result.is_err(), "tx should fail for player_num=7");
    println!("invalid_player_num passed — tx correctly rejected");
    Ok(())
}

/// Player reclaims their own note after expiry_block
#[tokio::test]
async fn expiry_recall() -> anyhow::Result<()> {
    let mut s = setup().await?;

    // Set expiry to block 1 (already passed in MockChain)
    let (n1, _n2) = make_bet_notes(&mut s, 20, 1, 2, 1, 0, BET_VALUE, 1)?;

    // Player1 consumes their own note (recall path)
    let tx_context = s
        .mock_chain
        .build_tx_context(s.player1.id(), &[n1.id()], &[])?
        .build()?;
    let executed = tx_context.execute().await?;

    // No output notes — assets go directly to player1's vault
    let output_notes: Vec<_> = executed.output_notes().iter().collect();
    assert_eq!(output_notes.len(), 0, "recall should produce no output notes");

    println!("expiry_recall passed — player reclaimed {BET_VALUE}");
    Ok(())
}

/// Second settlement attempt on the same round must fail
#[tokio::test]
async fn double_settlement() -> anyhow::Result<()> {
    let mut s = setup().await?;
    let (n1, n2) = make_bet_notes(&mut s, 30, 1, 2, 1, 0, BET_VALUE, EXPIRY)?;

    // First settlement succeeds
    let tx_context = s
        .mock_chain
        .build_tx_context(s.house.id(), &[n1.id(), n2.id()], &[])?
        .build()?;
    let executed = tx_context.execute().await?;
    s.mock_chain.add_pending_executed_transaction(&executed)?;
    s.mock_chain.prove_next_block()?;

    // Create a duplicate round with the same round_id (settled flag = 1)
    let (n3, n4) = make_bet_notes(&mut s, 30, 1, 2, 1, 0, BET_VALUE, EXPIRY)?;
    let tx_context2 = s
        .mock_chain
        .build_tx_context(s.house.id(), &[n3.id(), n4.id()], &[])?
        .build()?;
    let result = tx_context2.execute().await;

    assert!(result.is_err(), "second settlement for same round_id must fail");
    println!("double_settlement passed — replay correctly rejected");
    Ok(())
}

/// Notes with different faucet IDs must fail cross-validation
#[tokio::test]
async fn faucet_mismatch() -> anyhow::Result<()> {
    let mut s = setup().await?;

    // Create a second faucet
    let faucet2 = s.mock_chain.add_existing_faucet(Auth::NoAuth, "OTHER", 1_000_000_000_000)?;

    let faucet1_id = s.faucet.id();
    let faucet2_id = faucet2.id();

    let asset1 = Asset::Fungible(FungibleAsset::new(faucet1_id, BET_VALUE)?);
    let asset2 = Asset::Fungible(FungibleAsset::new(faucet2_id, BET_VALUE)?);

    let p1_id = s.player1.id();
    let p2_id = s.player2.id();
    let house_id = s.house.id();

    let inputs1 = make_note_inputs(40, 1, 1, 2, p1_id, p2_id, house_id, BET_VALUE, EXPIRY);
    let inputs2 = make_note_inputs(40, 2, 1, 0, p1_id, p2_id, house_id, BET_VALUE, EXPIRY);

    let n1 = create_testing_note_from_package(
        s.note_package.clone(), p1_id,
        NoteCreationConfig {
            note_type: NoteType::Private,
            assets: NoteAssets::new(vec![asset1])?,
            inputs: inputs1,
            ..Default::default()
        },
    )?;
    let n2 = create_testing_note_from_package(
        s.note_package.clone(), p2_id,
        NoteCreationConfig {
            note_type: NoteType::Private,
            assets: NoteAssets::new(vec![asset2])?,
            inputs: inputs2,
            ..Default::default()
        },
    )?;

    s.mock_chain.add_output_note(OutputNote::Full(n1.clone()));
    s.mock_chain.add_output_note(OutputNote::Full(n2.clone()));

    let tx_context = s
        .mock_chain
        .build_tx_context(s.house.id(), &[n1.id(), n2.id()], &[])?
        .build()?;
    let result = tx_context.execute().await;

    assert!(result.is_err(), "faucet mismatch must cause tx failure");
    println!("faucet_mismatch passed — tx correctly rejected");
    Ok(())
}
