// Do not link against libstd (i.e. anything defined in `std::`)
#![no_std]
#![feature(alloc_error_handler)]
extern crate alloc;
use alloc::vec;

use miden::{
    component, felt, Asset, Felt, NoteType, Recipient, StorageMap, StorageMapAccess, Tag, Word,
    native_account, output_note,
};

/// House account component for Morra game settlement.
///
/// Stores per-player-round game values (h, g, faucet, bet_value, commitment) and
/// handles deterministic payout note creation. The house cannot deviate from rules
/// embedded in the bet-note script.
#[component]
struct HouseAccount {
    /// Storage map holding all per-round game state.
    #[storage(description = "bets storage map")]
    bets: StorageMap,
}

#[component]
impl HouseAccount {
    /// Builds the storage key for a specific field of a player-round slot.
    ///
    /// Key layout: [round_id, player_num, field_idx, 0]
    fn player_key(round_id: Felt, player_num: Felt, field: Felt) -> Word {
        Word::from([round_id, player_num, field, felt!(0)])
    }

    /// Builds the storage key for the per-round settled flag.
    ///
    /// Key layout: [round_id, 0, 99, 0]
    fn settled_key(round_id: Felt) -> Word {
        Word::from([round_id, felt!(0), felt!(99), felt!(0)])
    }

    // ─── Write methods ────────────────────────────────────────────────────────

    /// Stores h, g, registered=1, faucet_suffix, faucet_prefix for a player-round.
    /// Called by the first bet-note in a settlement transaction.
    pub fn store_player_bet(
        &mut self,
        round_id: Felt,
        player_num: Felt,
        h: Felt,
        g: Felt,
        faucet_suffix: Felt,
        faucet_prefix: Felt,
    ) {
        self.bets.set(Self::player_key(round_id, player_num, felt!(0)), h);
        self.bets.set(Self::player_key(round_id, player_num, felt!(1)), g);
        self.bets.set(Self::player_key(round_id, player_num, felt!(2)), felt!(1)); // registered
        self.bets.set(Self::player_key(round_id, player_num, felt!(3)), faucet_suffix);
        self.bets.set(Self::player_key(round_id, player_num, felt!(4)), faucet_prefix);
    }

    /// Stores bet_value and player-ID XOR commitment for a player-round.
    /// Called immediately after store_player_bet by the first note.
    pub fn store_player_round_params(
        &mut self,
        round_id: Felt,
        player_num: Felt,
        bet_value: Felt,
        player_id_xor_s: Felt,
        player_id_xor_p: Felt,
    ) {
        self.bets.set(Self::player_key(round_id, player_num, felt!(5)), bet_value);
        self.bets.set(Self::player_key(round_id, player_num, felt!(6)), player_id_xor_s);
        self.bets.set(Self::player_key(round_id, player_num, felt!(7)), player_id_xor_p);
    }

    /// Clears all 8 storage fields for a player-round after settlement.
    /// Does NOT clear the settled flag.
    pub fn clear_player_bet(&mut self, round_id: Felt, player_num: Felt) {
        for field in 0u64..8 {
            self.bets.set(Self::player_key(round_id, player_num, Felt::new(field)), felt!(0));
        }
    }

    /// Marks a round as permanently settled. Never cleared.
    pub fn mark_round_settled(&mut self, round_id: Felt) {
        self.bets.set(Self::settled_key(round_id), felt!(1));
    }

    // ─── Read methods ─────────────────────────────────────────────────────────

    /// Returns 1 if player has registered their bet for this round, 0 otherwise.
    pub fn is_player_registered(&self, round_id: Felt, player_num: Felt) -> Felt {
        self.bets.get(&Self::player_key(round_id, player_num, felt!(2)))
    }

    pub fn get_player_h(&self, round_id: Felt, player_num: Felt) -> Felt {
        self.bets.get(&Self::player_key(round_id, player_num, felt!(0)))
    }

    pub fn get_player_g(&self, round_id: Felt, player_num: Felt) -> Felt {
        self.bets.get(&Self::player_key(round_id, player_num, felt!(1)))
    }

    pub fn get_player_faucet_suffix(&self, round_id: Felt, player_num: Felt) -> Felt {
        self.bets.get(&Self::player_key(round_id, player_num, felt!(3)))
    }

    pub fn get_player_faucet_prefix(&self, round_id: Felt, player_num: Felt) -> Felt {
        self.bets.get(&Self::player_key(round_id, player_num, felt!(4)))
    }

    pub fn get_player_bet_value(&self, round_id: Felt, player_num: Felt) -> Felt {
        self.bets.get(&Self::player_key(round_id, player_num, felt!(5)))
    }

    pub fn get_player_id_xor_s(&self, round_id: Felt, player_num: Felt) -> Felt {
        self.bets.get(&Self::player_key(round_id, player_num, felt!(6)))
    }

    pub fn get_player_id_xor_p(&self, round_id: Felt, player_num: Felt) -> Felt {
        self.bets.get(&Self::player_key(round_id, player_num, felt!(7)))
    }

    /// Returns 1 if this round has been settled, 0 if still open.
    pub fn is_round_settled(&self, round_id: Felt) -> Felt {
        self.bets.get(&Self::settled_key(round_id))
    }

    // ─── Payout note creation ─────────────────────────────────────────────────

    /// Returns the well-known P2ID note script root.
    ///
    /// TODO: Replace with the actual P2ID root from miden-standards once it is
    /// accessible as a constant in the contract SDK (use miden::*).
    fn p2id_note_root() -> Word {
        Word::from([felt!(0), felt!(0), felt!(0), felt!(0)])
    }

    /// Creates a private P2ID output note and removes `amount` from the house vault.
    ///
    /// Arguments (9 Felts — within the 4-Word/16-Felt ABI limit):
    ///   s0..s3      — note serial number components (unique per round × player)
    ///   amount      — payout amount in base token units
    ///   faucet_*    — identifies the fungible asset faucet
    ///   recipient_* — identifies the winner's account (suffix then prefix)
    pub fn create_payout_note(
        &mut self,
        s0: Felt,
        s1: Felt,
        s2: Felt,
        s3: Felt,
        amount: Felt,
        faucet_suffix: Felt,
        faucet_prefix: Felt,
        recipient_suffix: Felt,
        recipient_prefix: Felt,
    ) {
        let serial_num = Word::from([s0, s1, s2, s3]);
        let script_root = Self::p2id_note_root();
        let recipient =
            Recipient::compute(serial_num, script_root, vec![recipient_suffix, recipient_prefix]);
        let note_idx = output_note::create(Tag::from(felt!(0)), NoteType::Private, recipient);
        let asset = Asset::new(Word::from([amount, felt!(0), faucet_suffix, faucet_prefix]));
        native_account::remove_asset(asset.clone());
        output_note::add_asset(asset, note_idx);
    }
}
