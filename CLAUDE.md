# Alla Morra — AI Agent Instructions

This is a standalone Miden Morra (finger-guessing) game implemented as two smart contracts.

## Contract Overview

| Contract | Kind | Path |
|---|---|---|
| `house-account` | Account component | `contracts/house-account/` |
| `bet-note` | Note script | `contracts/bet-note/` |

The house account is a deterministic settlement facilitator. All game resolution logic lives in the bet-note script. The house cannot deviate from the rules.

## Build Order

**Always build house-account first** — bet-note depends on its generated WIT:

```bash
cargo miden build --manifest-path contracts/house-account/Cargo.toml --release
cargo miden build --manifest-path contracts/bet-note/Cargo.toml --release
```

## Tests

```bash
cd integration && cargo test -p integration --release -- morra
```

## Key Design Points

- **Storage keys**: `Word::from([round_id, player_num, field_idx, 0])` per player-round slot (8 fields, 0–7). Settled flag at `Word::from([round_id, 0, 99, 0])`.
- **Two-note atomicity**: First note stores game values; second note cross-validates and settles. Order-independent: each checks whether its *opponent* is already registered.
- **Payout math**: `fee = 2 * bet_value / 100` (1%). `bet_value` must be divisible by 50.
- **Recall path**: A player can reclaim their note after `expiry_block` if the house never settled.
- **XOR commitment**: `p1_suffix XOR p2_suffix` (and prefix) binds the player pair to a round. v2 should use RPO hash.

## Reference Patterns

For SDK patterns, pitfalls, and testing conventions, see the `agentic-template` repo:
- `project-template/contracts/counter-account/` — account component with StorageMap
- `project-template/contracts/increment-note/` — note script with cross-component calls
- `project-template/integration/tests/counter_test.rs` — MockChain test pattern

Use the `rust-sdk-patterns`, `rust-sdk-pitfalls`, and `rust-sdk-testing-patterns` skills for detailed guidance.

## CLI Binaries

Three binaries in `integration/src/bin/`:

| Binary | Role | Purpose |
|--------|------|---------|
| `setup_player --role house` | House | Deploy house account (once) |
| `setup_player` | Player | Create wallet + get faucet tokens (once per machine) |
| `publish_bet` | Player | Submit move for a round; prints serial_num for house |
| `settle_round` | House | Reconstruct notes from serials and settle |

`--data-dir` isolates keystore + store per player. Default: `..` (same as testnet integration binary).

## Known TODOs / v2

- `p2id_note_root()` in house-account is a placeholder. Replace with the actual P2ID script root from miden-standards once accessible as a constant via `use miden::*`.
- Move privacy: current notes are `NoteType::Public` — h/g are visible on-chain. v2 should use `NoteType::Private` with a commit-reveal scheme.
- Replace XOR commitment with RPO hash for cryptographic binding of player-pair to round.
- `Felt::as_int()` is the host-side method (miden-core Rust); `as_u64()` is the VM-side method. Don't confuse them in integration code.
