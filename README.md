# Alla Morra

A 2-player finger-guessing game ([Morra](https://en.wikipedia.org/wiki/Morra_(game))) running on [Miden](https://polygon.technology/miden). All game logic lives on-chain — the house cannot change the rules or steal funds.

## What is Morra?

Players simultaneously reveal a number of fingers (0–3) and guess the combined total (0–6). The player whose guess matches the total wins. If both guess correctly or both guess wrong, it's a draw.

## How the Contracts Work

**`house-account`** — An account component that stores per-round game state in a StorageMap and creates payout notes. The house is a settlement facilitator, not a trusted party.

**`bet-note`** — A note script that embeds all game logic. Each player creates a bet note with their move (`h`, `g`) and their wager. When the house consumes both notes in one transaction, the second note script reads the first player's stored values, computes the outcome, and emits payout note(s) — all verified by the Miden VM.

**Fee**: 1% of the pot, retained automatically in the house vault.

**Expiry**: If the house stalls, players get a refund after `expiry_block` (house-mediated recall).

---

## How to Play

Three roles: **Player 1**, **Player 2**, and a **house operator** who settles the round. One of the two players can also act as house operator — they just need to run one extra command at the end.

A house account is already deployed on testnet at:
```
HOUSE_ID=mtst1az7nmfpgwjpkcqpkm3fyuwwy7y9z43w3
FAUCET_ID=mtst1aqmat9m63ctdsgz6xcyzpuprpulwk9vg
```

### Prerequisites (both players, once)

```bash
git clone https://github.com/guelowrd/alla-morra
cd alla-morra
cargo build --release -p integration
```

You need Rust + the `cargo-miden` toolchain. See the [Miden docs](https://docs.polygon.technology/miden/) for setup.

---

### Step 1 — Each player: create your wallet (once)

Run this on your own machine:

```bash
cargo run --release --bin setup_player -- --data-dir ./my-data
```

This creates a wallet, requests testnet tokens from the faucet, and consumes them into your vault automatically. At the end it prints:

```
Account ID: mtst1<your-unique-id>      ← this is YOUR_PLAYER_ID
Faucet ID:  mtst1aqmat9m63ctdsgz6xcyzpuprpulwk9vg
```

**Save your Account ID.** Share it with the other player — you each need the other's ID for the next step.

---

### Step 2 — Off-chain: agree on game parameters

Before submitting your moves, both players agree on:

| Parameter | How to get it |
|-----------|--------------|
| `ROUND_ID` | Any number you both agree on (e.g. `42`). Must be unique per game. |
| `PLAYER1_ID` | Player 1's Account ID (printed in Step 1) |
| `PLAYER2_ID` | Player 2's Account ID (printed in Step 1) |
| `HOUSE_ID` | `mtst1az7nmfpgwjpkcqpkm3fyuwwy7y9z43w3` (deployed) |
| `FAUCET_ID` | `mtst1aqmat9m63ctdsgz6xcyzpuprpulwk9vg` |

---

### Step 3 — Each player: submit your move

Each player picks their move privately — `h` (fingers to show, 0–3) and `g` (guess of the total, 0–6) — and submits independently. **Don't share your move with the other player before submitting.**

**Player 1** — substitute your values for `<...>`:
```bash
cargo run --release --bin publish_bet -- \
  --round-id   <ROUND_ID> \
  --player-num 1 \
  --h <YOUR_FINGERS> --g <YOUR_GUESS> \
  --my-id      <PLAYER1_ID> \
  --player1-id <PLAYER1_ID> \
  --player2-id <PLAYER2_ID> \
  --house-id   mtst1az7nmfpgwjpkcqpkm3fyuwwy7y9z43w3 \
  --faucet-id  mtst1aqmat9m63ctdsgz6xcyzpuprpulwk9vg \
  --data-dir   ./my-data
```

**Player 2** — same command, with `--player-num 2` and your own `--my-id`:
```bash
cargo run --release --bin publish_bet -- \
  --round-id   <ROUND_ID> \
  --player-num 2 \
  --h <YOUR_FINGERS> --g <YOUR_GUESS> \
  --my-id      <PLAYER2_ID> \
  --player1-id <PLAYER1_ID> \
  --player2-id <PLAYER2_ID> \
  --house-id   mtst1az7nmfpgwjpkcqpkm3fyuwwy7y9z43w3 \
  --faucet-id  mtst1aqmat9m63ctdsgz6xcyzpuprpulwk9vg \
  --data-dir   ./my-data
```

Each command prints a **serial number** at the end:
```
--p1-serial <64 hex chars>
--p2-serial <64 hex chars>
```

**Now reveal your moves and serials** — both players send the house operator their `h`, `g`, and serial.

---

### Step 4 — House operator: settle the round

The house operator runs this once both serials are received (substitute all `<...>` with the actual values shared by players):

```bash
cargo run --release --bin settle_round -- \
  --round-id   <ROUND_ID> \
  --house-id   mtst1az7nmfpgwjpkcqpkm3fyuwwy7y9z43w3 \
  --faucet-id  mtst1aqmat9m63ctdsgz6xcyzpuprpulwk9vg \
  --player1-id <PLAYER1_ID> \
  --p1-h <P1_FINGERS> --p1-g <P1_GUESS> \
  --p1-serial  <P1_SERIAL> \
  --player2-id <PLAYER2_ID> \
  --p2-h <P2_FINGERS> --p2-g <P2_GUESS> \
  --p2-serial  <P2_SERIAL> \
  --data-dir   ./house-data
```

The Miden VM verifies the outcome and emits payout notes. Example output:

```
Settlement complete!
Tx:      0x30bbafa28e73c064d4753b717cbffc869c66a52d6d9a1d17e42016ba20ccc1e2
Outcome: DRAW — 990000 base units each
```

> **Who is the house operator?** If no dedicated operator exists, Player 1 can take this role — they just need the `./house-data` directory (keystore + store) from whoever deployed the house account. For the deployed house above, contact the repo owner.

---

## Payout Rules

| Outcome | Who wins | Payout |
|---------|----------|--------|
| Only P1 guessed right | Player 1 | 1.98× bet |
| Only P2 guessed right | Player 2 | 1.98× bet |
| Both right or both wrong | Draw | 0.99× bet each |

1% of the pot is retained as a house fee.

---

## CLI Reference

| Binary | Who | When |
|--------|-----|------|
| `setup_player --role house` | House operator | Once, at deployment |
| `setup_player` | Each player | Once per machine |
| `publish_bet` | Each player | Once per round |
| `settle_round` | House operator | Once per round |

**Common flags:**

| Flag | Default | Notes |
|------|---------|-------|
| `--bet-value` | `1000000` | Base units, must be divisible by 50 |
| `--expiry-block` | `999999` | Block after which refund can be requested |
| `--data-dir` | `..` | Directory for your keystore + SQLite store |

---

## Build & Test

```bash
# Build contracts (house-account first — bet-note depends on its WIT)
cargo miden build --manifest-path contracts/house-account/Cargo.toml --release
cargo miden build --manifest-path contracts/bet-note/Cargo.toml --release

# Run MockChain integration tests (10 test cases)
# Do not add a test-name filter — test names are p1_wins, p2_wins, etc.
cargo test -p integration --release

# If you see stale compile errors, clean first:
cargo clean -p integration && cargo test -p integration --release

# Build all CLI binaries
cargo build --release -p integration
```

---

## Architecture

See [ARCHITECTURE_PLAN.md](./ARCHITECTURE_PLAN.md) for the full design: storage schema, 14 component methods, protocol requirements, payout math, and known limitations.
