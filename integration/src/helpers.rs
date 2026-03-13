//! Common helper functions for scripts and tests

use std::{borrow::Borrow, collections::BTreeSet, path::Path, sync::Arc, time::Duration};

use anyhow::{bail, Context, Result};
use cargo_miden::{run, OutputType};
use miden_client::{
    account::{
        component::{AccountComponentMetadata, AuthFalcon512Rpo, BasicWallet, NoAuth},
        Account, AccountBuilder, AccountComponent, AccountId, AccountStorageMode, AccountType,
        StorageSlot,
    },
    asset::{Asset, FungibleAsset},
    auth::{AuthSecretKey, PublicKeyCommitment},
    block::BlockNumber,
    builder::ClientBuilder,
    crypto::{rpo_falcon512::SecretKey, FeltRng},
    keystore::FilesystemKeyStore,
    note::{Note, NoteAssets, NoteInputs, NoteMetadata, NoteRecipient, NoteScript, NoteTag, NoteType},
    rpc::{Endpoint, GrpcClient},
    utils::Deserializable,
    Client, Word,
};
use miden_client_sqlite_store::ClientBuilderSqliteExt;
use miden_core::Felt;
use miden_mast_package::{Package, SectionId};
use rand::RngCore;
use sha2::{Digest as Sha2Digest, Sha256};

/// Test setup configuration containing initialized client and keystore
pub struct ClientSetup {
    pub client: Client<FilesystemKeyStore>,
    pub keystore: Arc<FilesystemKeyStore>,
}

/// Initializes test infrastructure with client and keystore (default paths: `../keystore`, `../store.sqlite3`).
pub async fn setup_client() -> Result<ClientSetup> {
    setup_client_at(Path::new("..")).await
}

/// Initializes client and keystore with explicit data directory.
/// Creates `{data_dir}/keystore/` and `{data_dir}/store.sqlite3`.
pub async fn setup_client_at(data_dir: &Path) -> Result<ClientSetup> {
    let endpoint = Endpoint::testnet();
    let timeout_ms = 10_000;
    let rpc_client = Arc::new(GrpcClient::new(&endpoint, timeout_ms));

    let keystore_path = data_dir.join("keystore");
    let keystore =
        Arc::new(FilesystemKeyStore::new(keystore_path).context("Failed to initialize keystore")?);

    let store_path = data_dir.join("store.sqlite3");

    let client = ClientBuilder::new()
        .rpc(rpc_client)
        .sqlite_store(store_path)
        .authenticator(keystore.clone())
        .in_debug_mode(true.into())
        .build()
        .await
        .context("Failed to build Miden client")?;

    Ok(ClientSetup { client, keystore })
}

/// Builds a Miden project in the specified directory and returns the compiled Package.
pub fn build_project_in_dir(dir: &Path, release: bool) -> Result<Package> {
    let profile = if release { "--release" } else { "--debug" };
    let manifest_path = dir.join("Cargo.toml");
    let manifest_arg = manifest_path.to_string_lossy();

    let args = vec![
        "cargo",
        "miden",
        "build",
        profile,
        "--manifest-path",
        &manifest_arg,
    ];

    let output = run(args.into_iter().map(String::from), OutputType::Masm)
        .context("Failed to compile project")?
        .context("Cargo miden build returned None")?;

    let artifact_path = match output {
        cargo_miden::CommandOutput::BuildCommandOutput { output } => match output {
            cargo_miden::BuildOutput::Masm { artifact_path } => artifact_path,
            other => bail!("Expected Masm output, got {:?}", other),
        },
        other => bail!("Expected BuildCommandOutput, got {:?}", other),
    };

    let package_bytes = std::fs::read(&artifact_path).context(format!(
        "Failed to read compiled package from {}",
        artifact_path.display()
    ))?;

    Package::read_from_bytes(&package_bytes).context("Failed to deserialize package from bytes")
}

/// Configuration for creating an account with a custom component
#[derive(Clone)]
pub struct AccountCreationConfig {
    pub account_type: AccountType,
    pub storage_mode: AccountStorageMode,
    pub storage_slots: Vec<StorageSlot>,
    pub supported_types: Option<Vec<AccountType>>,
}

impl Default for AccountCreationConfig {
    fn default() -> Self {
        Self {
            account_type: AccountType::RegularAccountImmutableCode,
            storage_mode: AccountStorageMode::Public,
            storage_slots: vec![],
            supported_types: None,
        }
    }
}

/// Creates an AccountComponent from a compiled package.
pub fn account_component_from_package(
    package: Arc<Package>,
    config: &AccountCreationConfig,
) -> Result<AccountComponent> {
    let account_component_metadata = package.sections.iter().find_map(|s| {
        if s.id == SectionId::ACCOUNT_COMPONENT_METADATA {
            Some(s.data.borrow())
        } else {
            None
        }
    });

    let account_component = match account_component_metadata {
        None => bail!("Package missing account component metadata"),
        Some(bytes) => {
            let metadata = AccountComponentMetadata::read_from_bytes(bytes)
                .context("Failed to deserialize account component metadata")?;

            let component = AccountComponent::new(
                package.unwrap_library().as_ref().clone(),
                config.storage_slots.clone(),
            )
            .context("Failed to create account component")?
            .with_metadata(metadata);

            let supported_types = if let Some(types) = &config.supported_types {
                BTreeSet::from_iter(types.clone())
            } else {
                BTreeSet::from_iter([AccountType::RegularAccountImmutableCode])
            };

            component.with_supported_types(supported_types)
        }
    };

    Ok(account_component)
}

/// Creates an account from a compiled package and registers it with the live client.
pub async fn create_account_from_package(
    client: &mut Client<FilesystemKeyStore>,
    package: Arc<Package>,
    config: AccountCreationConfig,
) -> Result<Account> {
    let account_component = account_component_from_package(package, &config)
        .context("Failed to create account component from package")?;

    let mut init_seed = [0_u8; 32];
    client.rng().fill_bytes(&mut init_seed);

    let account = AccountBuilder::new(init_seed)
        .account_type(config.account_type)
        .storage_mode(config.storage_mode)
        .with_component(account_component)
        .with_auth_component(NoAuth)
        .build()
        .context("Failed to build account")?;

    client
        .add_account(&account, false)
        .await
        .context("Failed to add account to client")?;

    Ok(account)
}

/// Creates a note with a random serial number using the client's RNG.
pub fn create_note_from_package(
    client: &mut Client<FilesystemKeyStore>,
    package: Arc<Package>,
    sender_id: AccountId,
    config: NoteCreationConfig,
) -> Result<Note> {
    let note_program = package.unwrap_program();
    let note_script = NoteScript::from_parts(
        note_program.mast_forest().clone(),
        note_program.entrypoint(),
    );

    let serial_num = client.rng().draw_word();
    let note_inputs = NoteInputs::new(config.inputs).context("Failed to create note inputs")?;
    let recipient = NoteRecipient::new(serial_num, note_script, note_inputs);
    let metadata = NoteMetadata::new(sender_id, config.note_type, config.tag);

    Ok(Note::new(config.assets, metadata, recipient))
}

/// Creates a deterministic test account from a compiled package (no live client needed).
pub async fn create_testing_account_from_package(
    package: Arc<Package>,
    config: AccountCreationConfig,
) -> Result<Account> {
    let account_component = account_component_from_package(package, &config)
        .context("Failed to create account component from package")?;

    let account = AccountBuilder::new([3u8; 32])
        .account_type(config.account_type)
        .storage_mode(config.storage_mode)
        .with_component(account_component)
        .with_auth_component(NoAuth)
        .build_existing()
        .context("Failed to build account")?;

    Ok(account)
}

/// Configuration for creating a note
pub struct NoteCreationConfig {
    pub note_type: NoteType,
    pub tag: NoteTag,
    pub assets: miden_client::note::NoteAssets,
    pub inputs: Vec<Felt>,
}

impl Default for NoteCreationConfig {
    fn default() -> Self {
        Self {
            note_type: NoteType::Public,
            tag: NoteTag::new(0),
            assets: Default::default(),
            inputs: Default::default(),
        }
    }
}

/// Creates a deterministic test note from a compiled package.
pub fn create_testing_note_from_package(
    package: Arc<Package>,
    sender_id: AccountId,
    config: NoteCreationConfig,
) -> Result<Note> {
    let note_program = package.unwrap_program();
    let note_script = NoteScript::from_parts(
        note_program.mast_forest().clone(),
        note_program.entrypoint(),
    );

    let random_u64s = [0_u64; 4];
    let serial_num =
        Word::try_from(random_u64s).context("Failed to convert random u64s to word")?;

    let note_inputs = NoteInputs::new(config.inputs).context("Failed to create note inputs")?;
    let recipient = NoteRecipient::new(serial_num, note_script, note_inputs);

    let metadata = NoteMetadata::new(sender_id, config.note_type, config.tag);

    Ok(Note::new(config.assets, metadata, recipient))
}

// ── Shared live-client types ──────────────────────────────────────────────────

pub type MidenClient = Client<FilesystemKeyStore>;

// ── Faucet helpers ────────────────────────────────────────────────────────────

pub const FAUCET_API: &str = "https://faucet-api-testnet-miden.eu-central-8.gateway.fm";

#[derive(serde::Deserialize)]
pub struct FaucetMeta {
    pub id: String,
    pub base_amount: u64,
    pub pow_load_difficulty: u64,
}

#[derive(serde::Deserialize)]
struct PowChallenge {
    challenge: String,
    target: u64,
}

pub async fn faucet_meta(http: &reqwest::Client) -> Result<FaucetMeta> {
    Ok(http
        .get(format!("{FAUCET_API}/get_metadata"))
        .send()
        .await?
        .json()
        .await?)
}

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
/// Solves the PoW challenge automatically.
pub async fn request_faucet_tokens(
    http: &reqwest::Client,
    account_id_bech32: &str,
    amount: u64,
    meta: &FaucetMeta,
) -> Result<String> {
    let pow: PowChallenge = http
        .get(format!("{FAUCET_API}/pow"))
        .query(&[("account_id", account_id_bech32), ("amount", &amount.to_string())])
        .send()
        .await?
        .json()
        .await?;
    println!("    PoW target: {} (difficulty ~{})", pow.target, meta.pow_load_difficulty);

    let nonce = solve_pow(&pow.challenge, pow.target);
    println!("    Solved with nonce: {nonce}");

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

/// Syncs repeatedly until at least one new block has been committed.
pub async fn wait_for_new_block(
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

/// Syncs repeatedly until `get_consumable_notes` returns at least one note for `account_id`.
pub async fn wait_for_consumable_note(
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

// ── Note input helpers ────────────────────────────────────────────────────────

/// Returns (suffix_felt, prefix_felt) for an AccountId.
pub fn id_felts(id: AccountId) -> (Felt, Felt) {
    (id.suffix(), id.prefix().into())
}

/// Builds the 12-Felt input vector for a bet note.
pub fn make_note_inputs(
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

/// Encodes a Word (4 × Felt) as a 64-char lowercase hex string.
pub fn encode_word(word: Word) -> String {
    let bytes: Vec<u8> = word.iter().flat_map(|f| f.as_int().to_le_bytes()).collect();
    hex::encode(bytes)
}

/// Decodes a 64-char hex string back into a Word.
pub fn decode_word(hex_str: &str) -> Result<Word> {
    let bytes = hex::decode(hex_str).context("invalid hex in serial")?;
    anyhow::ensure!(bytes.len() == 32, "serial must be 64 hex chars (got {})", hex_str.len());
    let u64s = [
        u64::from_le_bytes(bytes[0..8].try_into()?),
        u64::from_le_bytes(bytes[8..16].try_into()?),
        u64::from_le_bytes(bytes[16..24].try_into()?),
        u64::from_le_bytes(bytes[24..32].try_into()?),
    ];
    Word::try_from(u64s).map_err(|e| anyhow::anyhow!("invalid word: {:?}", e))
}

/// Reconstructs a bet Note from game parameters and a previously recorded serial_num.
/// The resulting Note has the same ID as the one originally created and published.
pub fn reconstruct_bet_note(
    note_package: &Package,
    player_num: u64,
    h: u64,
    g: u64,
    sender_id: AccountId,
    serial_hex: &str,
    round_id: u64,
    p1_id: AccountId,
    p2_id: AccountId,
    house_id: AccountId,
    faucet_id: AccountId,
    bet_value: u64,
    expiry_block: u64,
) -> Result<Note> {
    let serial_num = decode_word(serial_hex)?;
    let note_program = note_package.unwrap_program();
    let note_script = NoteScript::from_parts(
        note_program.mast_forest().clone(),
        note_program.entrypoint(),
    );
    let inputs = NoteInputs::new(make_note_inputs(
        round_id, player_num, h, g, p1_id, p2_id, house_id, bet_value, expiry_block,
    ))
    .context("Failed to create note inputs")?;
    let recipient = NoteRecipient::new(serial_num, note_script, inputs);
    let metadata = NoteMetadata::new(sender_id, NoteType::Public, NoteTag::new(0));
    let asset = Asset::Fungible(FungibleAsset::new(faucet_id, bet_value)?);
    let assets = NoteAssets::new(vec![asset]).context("Failed to create note assets")?;
    Ok(Note::new(assets, metadata, recipient))
}

/// Creates a basic wallet account with authentication (for live client use).
pub async fn create_basic_wallet_account(
    client: &mut Client<FilesystemKeyStore>,
    keystore: Arc<FilesystemKeyStore>,
    config: AccountCreationConfig,
) -> Result<Account> {
    let mut init_seed = [0_u8; 32];
    client.rng().fill_bytes(&mut init_seed);

    let key_pair = SecretKey::with_rng(client.rng());

    let builder = AccountBuilder::new(init_seed)
        .account_type(config.account_type)
        .storage_mode(config.storage_mode)
        .with_auth_component(AuthFalcon512Rpo::new(PublicKeyCommitment::from(
            key_pair.public_key().to_commitment(),
        )))
        .with_component(BasicWallet);

    let account = builder
        .build()
        .context("Failed to build basic wallet account")?;

    client
        .add_account(&account, false)
        .await
        .context("Failed to add account to client")?;

    keystore
        .add_key(&AuthSecretKey::Falcon512Rpo(key_pair))
        .context("Failed to add key to keystore")?;

    Ok(account)
}
