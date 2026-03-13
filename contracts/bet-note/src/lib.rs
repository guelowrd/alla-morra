// Do not link against libstd (i.e. anything defined in `std::`)
#![no_std]
#![feature(alloc_error_handler)]

use miden::*;

use crate::bindings::miden::house_account::house_account;

#[note]
struct BetNote;

#[note]
impl BetNote {
    /// Morra bet-note script.
    ///
    /// Note inputs layout (12 Felts):
    ///   [0]  round_id          — house-generated, globally unique per game
    ///   [1]  player_num        — must be exactly 1 or 2
    ///   [2]  h                 — fingers shown (0–3)
    ///   [3]  g                 — guess of total (0–6)
    ///   [4]  player1_id_suffix
    ///   [5]  player1_id_prefix
    ///   [6]  player2_id_suffix
    ///   [7]  player2_id_prefix
    ///   [8]  house_id_suffix
    ///   [9]  house_id_prefix
    ///   [10] bet_value         — must be divisible by 50
    ///   [11] expiry_block      — block after which the player can self-reclaim
    ///
    /// Both players' notes carry identical inputs except for player_num, h, and g.
    #[note_script]
    fn run(self, _arg: Word) {
        let inputs = active_note::get_inputs();
        let round_id     = inputs[0];
        let player_num   = inputs[1];
        let h            = inputs[2];
        let g            = inputs[3];
        let p1_suffix    = inputs[4];
        let p1_prefix    = inputs[5];
        let p2_suffix    = inputs[6];
        let p2_prefix    = inputs[7];
        let house_suffix = inputs[8];
        let house_prefix = inputs[9];
        let bet_value    = inputs[10];
        let expiry_block = inputs[11];

        // --- Validate player_num ∈ {1, 2} ---
        let pn = player_num.as_u64();
        assert(Felt::from_u64_unchecked((pn == 1 || pn == 2) as u64));

        // --- Validate h ∈ {0,1,2,3} and g ∈ {0,1,2,3,4,5,6} ---
        assert(Felt::from_u64_unchecked((h.as_u64() <= 3) as u64));
        assert(Felt::from_u64_unchecked((g.as_u64() <= 6) as u64));

        // --- Validate exactly one asset with the correct amount ---
        let assets = active_note::get_assets();
        assert_eq(Felt::from_u64_unchecked(assets.len() as u64), felt!(1));
        assert_eq(assets[0].inner[0], bet_value);
        let faucet_suffix = assets[0].inner[2];
        let faucet_prefix = assets[0].inner[3];

        // --- Branch: house settles or processes expiry; non-house forbidden ---
        let account_id = active_account::get_id();
        let is_house = account_id.suffix.as_u64() == house_suffix.as_u64()
                    && account_id.prefix.as_u64() == house_prefix.as_u64();

        // Only the house account may consume bet-notes.
        assert(Felt::from_u64_unchecked(is_house as u64));

        // Transfer note asset into the house vault (requires account component context).
        let a = assets[0];
        house_account::receive_asset(a.inner[0], a.inner[1], a.inner[2], a.inner[3]);

        // Check if this note has expired — house handles both paths.
        let current_block = tx::get_block_number();
        let is_expired = current_block.as_u64() > expiry_block.as_u64();

        if is_expired {
            // ── Expiry-recall path: house refunds the player ─────────────────
            let my_suffix = if pn == 1 { p1_suffix } else { p2_suffix };
            let my_prefix = if pn == 1 { p1_prefix } else { p2_prefix };
            house_account::create_payout_note(
                round_id, felt!(3), felt!(0), felt!(0),
                bet_value,
                faucet_suffix, faucet_prefix,
                my_suffix, my_prefix,
            );
        } else {
        // ── Settlement path ──────────────────────────────────────────────

            // Guard: fail immediately if this round was already settled
            assert_eq(house_account::is_round_settled(round_id), felt!(0));

            let opp_num = if pn == 1 { felt!(2) } else { felt!(1) };
            let opp_registered = house_account::is_player_registered(round_id, opp_num);

            // Participant commitment: XOR of both player ID components.
            // Prevents accidental note mismatches. v2 should use RPO hash.
            let id_xor_s = Felt::from_u64_unchecked(p1_suffix.as_u64() ^ p2_suffix.as_u64());
            let id_xor_p = Felt::from_u64_unchecked(p1_prefix.as_u64() ^ p2_prefix.as_u64());

            if opp_registered.as_u64() == 0 {
                // First note in this tx: store game values and round params.
                // Opponent has not yet registered — we go first.
                house_account::store_player_bet(
                    round_id, player_num, h, g, faucet_suffix, faucet_prefix,
                );
                house_account::store_player_round_params(
                    round_id, player_num, bet_value, id_xor_s, id_xor_p,
                );
            } else {
                // Second note in this tx: cross-validate all shared round params,
                // compute outcome, and emit payout note(s).

                // Cross-validate faucet identity
                let stored_faucet_s = house_account::get_player_faucet_suffix(round_id, opp_num);
                let stored_faucet_p = house_account::get_player_faucet_prefix(round_id, opp_num);
                assert_eq(faucet_suffix, stored_faucet_s);
                assert_eq(faucet_prefix, stored_faucet_p);

                // Cross-validate bet_value
                let stored_bv = house_account::get_player_bet_value(round_id, opp_num);
                assert_eq(bet_value, stored_bv);

                // Cross-validate player ID commitment
                let stored_xor_s = house_account::get_player_id_xor_s(round_id, opp_num);
                let stored_xor_p = house_account::get_player_id_xor_p(round_id, opp_num);
                assert_eq(id_xor_s, stored_xor_s);
                assert_eq(id_xor_p, stored_xor_p);

                // Retrieve opponent's game values
                let opp_h = house_account::get_player_h(round_id, opp_num);
                let opp_g = house_account::get_player_g(round_id, opp_num);

                // Assign h1/g1/h2/g2 from player perspective
                let (h1, g1, h2, g2) = if pn == 1 {
                    (h, g, opp_h, opp_g)
                } else {
                    (opp_h, opp_g, h, g)
                };

                // Compute outcome using u64 to avoid Felt modular arithmetic
                let total = h1.as_u64() + h2.as_u64();
                let p1_wins = g1.as_u64() == total;
                let p2_wins = g2.as_u64() == total;

                // Payout math — bet_value must be divisible by 50 for exact fee math
                let bv = bet_value.as_u64();
                let total_pot = bv * 2;
                let fee = total_pot / 100;           // 1% house fee
                let winner_payout = total_pot - fee; // e.g. 1.98 MIDEN
                let draw_payout = winner_payout / 2; // e.g. 0.99 MIDEN each

                // Payout serial numbers: deterministic, unique per round × player
                //   P1 note: [round_id, 1, 0, 0]
                //   P2 note: [round_id, 2, 0, 0]
                let s0 = round_id;

                if p1_wins && !p2_wins {
                    house_account::create_payout_note(
                        s0, felt!(1), felt!(0), felt!(0),
                        Felt::from_u64_unchecked(winner_payout),
                        faucet_suffix, faucet_prefix,
                        p1_suffix, p1_prefix,
                    );
                } else if p2_wins && !p1_wins {
                    house_account::create_payout_note(
                        s0, felt!(2), felt!(0), felt!(0),
                        Felt::from_u64_unchecked(winner_payout),
                        faucet_suffix, faucet_prefix,
                        p2_suffix, p2_prefix,
                    );
                } else {
                    // Draw: both correct or both wrong — split evenly
                    house_account::create_payout_note(
                        s0, felt!(1), felt!(0), felt!(0),
                        Felt::from_u64_unchecked(draw_payout),
                        faucet_suffix, faucet_prefix,
                        p1_suffix, p1_prefix,
                    );
                    house_account::create_payout_note(
                        s0, felt!(2), felt!(0), felt!(0),
                        Felt::from_u64_unchecked(draw_payout),
                        faucet_suffix, faucet_prefix,
                        p2_suffix, p2_prefix,
                    );
                }

                // Mark round permanently settled and clean up opponent's storage slot.
                // Current player's slot was never written (went straight to settlement).
                house_account::mark_round_settled(round_id);
                house_account::clear_player_bet(round_id, opp_num);
            }
        }
    }
}
