# Morra on Miden — Architecture Plan

## Overview

A 2-player finger-guessing game (Morra) implemented as two Miden smart contracts:

- **`house-account`**: Account component that serves as a deterministic settlement facilitator
- **`bet-note`**: Note script that embeds full game resolution logic

The house cannot deviate from the rules; all payouts are computed by the note script from the players' private values (h, g).

---

## Directory Structure

```
alla-morra/
├── contracts/
│   ├── house-account/       ← account component
│   └── bet-note/            ← note script (depends on house-account WIT)
├── integration/
│   ├── Cargo.toml
│   ├── src/
│   │   └── helpers.rs
│   └── tests/
│       └── morra_test.rs
├── Cargo.toml               ← workspace
├── ARCHITECTURE_PLAN.md     ← this file
├── CLAUDE.md                ← AI agent instructions
└── README.md
```

---

## Contract 1: `house-account`

### Storage Schema

Per-player-round (8 Felt slots per player):

| Key `Word::from([round_id, player_num, field, 0])` | Field | Stored value |
|---|---|---|
| field=0 | h | fingers shown (0–3) |
| field=1 | g | guess of total (0–6) |
| field=2 | registered | 0 or 1 |
| field=3 | faucet_suffix | fungible asset faucet ID (low) |
| field=4 | faucet_prefix | fungible asset faucet ID (high) |
| field=5 | bet_value | wager amount in base units |
| field=6 | id_xor_s | p1_suffix XOR p2_suffix |
| field=7 | id_xor_p | p1_prefix XOR p2_prefix |

Per-round settled flag (permanent):

| Key `Word::from([round_id, 0, 99, 0])` | Stored value |
|---|---|
| — | 0 = open, 1 = settled |

### Methods

14 public methods:
- `store_player_bet` / `store_player_round_params` — write game values (first note)
- `is_player_registered` / `get_player_*` — read game values (second note)
- `clear_player_bet` — zero out player slot after settlement
- `is_round_settled` / `mark_round_settled` — round lifecycle
- `create_payout_note` — create a private P2ID output note, remove asset from house vault

---

## Contract 2: `bet-note`

### Note Inputs (12 Felts)

```
[0]  round_id        house-generated, globally unique
[1]  player_num      1 or 2
[2]  h               fingers shown (0–3)
[3]  g               guess of total (0–6)
[4]  player1_suffix
[5]  player1_prefix
[6]  player2_suffix
[7]  player2_prefix
[8]  house_suffix
[9]  house_prefix
[10] bet_value       must be divisible by 50
[11] expiry_block    block after which player can self-reclaim
```

### Execution Paths

**Settlement path** (consuming account = house):
1. Check round not already settled
2. If opponent not yet registered → store game values (first note)
3. If opponent registered → cross-validate faucet/bet_value/XOR commitment, compute outcome, emit payout note(s), mark settled, clear opponent slot

**Recall path** (consuming account = player, after expiry):
1. Validate consuming account is the correct player
2. Validate `current_block > expiry_block`
3. Assets transfer automatically to player's vault

### Payout Math

```
total_pot     = 2 × bet_value
fee           = total_pot / 100        (1% house fee)
winner_payout = total_pot - fee        (e.g. 1.98 MIDEN for 1 MIDEN bet)
draw_payout   = winner_payout / 2      (e.g. 0.99 MIDEN each)
```

`bet_value` must be divisible by 50 so `fee` is an integer.

---

## Protocol Requirements

### round_id
Mission-critical. Provides storage isolation, serial uniqueness, and round pairing. House generates it (e.g. hash of player IDs + nonce) and communicates to both players before note creation. One round_id → one note pair → one settlement → permanent settled flag.

### Execution ordering
Order-independent. Each note checks whether its *opponent* is registered. Either can run first; settlement fires when the second note runs.

### Two-note atomicity
No on-chain mechanism enforces both notes in the same transaction. Mitigation:
- **Expiry/recall** (on-chain): players reclaim after `expiry_block` if house stalls
- **Known residual risk**: house consuming note1 alone leaves player1 stuck until expiry. Protocol violation, not passive failure. Out of scope for MVP.

### XOR commitment
`p1_suffix XOR p2_suffix` (and prefix) is stored by the first note and verified by the second. Not cryptographically binding. **v2: replace with RPO hash**.

---

## Known Limitations & TODOs

1. **`p2id_note_root()`** — placeholder in house-account. Needs the actual P2ID MAST root from miden-standards.
2. **`id_felts()` in tests** — uses `suffix().as_felt()` / `prefix().as_felt()`. Verify against miden-objects 0.20 API.
3. **XOR commitment** — upgrade to RPO hash in v2.
4. **`assets.len()`** — confirm `active_note::get_assets()` returns a type with `.len()` in no_std.
5. **Intra-tx state sharing** — verified assumption: storage writes from note 1 are visible to note 2 in the same tx. If this fails, settlement logic must move to a transaction script.
6. **Single-note transactions** — house consuming only one note leaves opponent storage dirty. `clear_player_bet` should be added to a timeout/cleanup path in v2.

---

## Test Coverage

| Test | h1 | g1 | h2 | g2 | Expected |
|---|---|---|---|---|---|
| `p1_wins` | 1 | 2 | 1 | 0 | 1 note → P1, 1.98 MIDEN |
| `p2_wins` | 1 | 0 | 1 | 2 | 1 note → P2, 1.98 MIDEN |
| `draw_both_correct` | 1 | 2 | 1 | 2 | 2 notes → P1+P2, 0.99 each |
| `draw_both_wrong` | 1 | 0 | 1 | 1 | 2 notes → P1+P2, 0.99 each |
| `invalid_h` | 4 | — | — | — | tx fails |
| `invalid_g` | — | 7 | — | — | tx fails |
| `invalid_player_num` | — | — | — | — | tx fails (player_num=7) |
| `expiry_recall` | — | — | — | — | player reclaims after expiry |
| `double_settlement` | — | — | — | — | second tx fails (settled=1) |
| `faucet_mismatch` | — | — | — | — | tx fails (different faucets) |
