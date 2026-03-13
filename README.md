# Alla Morra

A 2-player finger-guessing game (Morra) on [Miden](https://polygon.technology/miden).

## What is Morra?

Players simultaneously reveal a number of fingers (0–3) and guess the combined total (0–6). The player whose guess matches the total wins. If both guess correctly or both guess wrong, the round is a draw.

## How the Contracts Work

Two contracts, fully deterministic:

**`house-account`** — An account component that stores per-round game state in a StorageMap and creates payout notes. The house is a settlement facilitator, not a trusted party — it cannot change the rules.

**`bet-note`** — A note script that embeds all game logic. Each player creates a bet note with their move (h, g) and wager. When the house consumes both notes in a single transaction, the second note script reads the first player's stored values, computes the outcome, and emits payout note(s) — all verified on-chain.

**Fee**: 1% of the pot. Stays in the house vault automatically.

**Expiry**: If the house stalls, players can reclaim their bet after `expiry_block`.

## Build

```bash
# Build in dependency order (house-account first)
cargo miden build --manifest-path contracts/house-account/Cargo.toml --release
cargo miden build --manifest-path contracts/bet-note/Cargo.toml --release
```

## Test

```bash
cargo test -p integration --release -- morra
```

## Architecture

See [ARCHITECTURE_PLAN.md](./ARCHITECTURE_PLAN.md) for the full design: storage schema, protocol requirements, payout math, and known limitations.
