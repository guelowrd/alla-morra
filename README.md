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

## Playing the Game

### Prerequisites

```bash
git clone https://github.com/guelowrd/alla-morra
cd alla-morra
cargo build --release -p integration
```

You need Rust + the `cargo-miden` toolchain. See the [Miden docs](https://docs.polygon.technology/miden/) for setup.

### Step 1 — House operator: deploy the house account (once)

```bash
cargo run --release --bin setup_player -- --role house --data-dir ./house-data
```

Prints a house account ID. Share it with both players.

```
House ID: mtst1ar9fdarcuvk0zqrnc44xdfmxp5yusalf
```

### Step 2 — Each player: set up a wallet and get testnet tokens (once per player)

Run this on your own machine with your own `--data-dir`:

```bash
cargo run --release --bin setup_player -- --data-dir ./my-data
```

Automatically:
- Creates a BasicWallet account on testnet
- Solves the faucet PoW challenge and requests tokens
- Consumes the faucet note (tokens land in vault)

Prints your account ID and faucet ID — save them.

```
Account ID: mtst1az6qut6g4z8ucqqllwaups53ngcpdgks
Faucet ID:  mtst1aqmat9m63ctdsgz6xcyzpuprpulwk9vg
```

### Step 3 — Each player: publish your move

Coordinate off-chain: agree on a `round-id` (any unique number), and share your account IDs with each other.

Then each player independently submits their move:

**Player 1** (shows 2 fingers, guesses total = 3):
```bash
cargo run --release --bin publish_bet -- \
  --round-id 100 --player-num 1 --h 2 --g 3 \
  --my-id     mtst1az6qut6g4z8ucqqllwaups53ngcpdgks \
  --player1-id mtst1az6qut6g4z8ucqqllwaups53ngcpdgks \
  --player2-id mtst1apekvcdafp5ekqrfhk2knm7yyyww4mlw \
  --house-id  mtst1ar9fdarcuvk0zqrnc44xdfmxp5yusalf \
  --faucet-id mtst1aqmat9m63ctdsgz6xcyzpuprpulwk9vg \
  --data-dir ./my-data
```

Prints a **serial number** (64 hex chars). Send it to the house operator — it's the only thing they need from you.

```
--p1-serial 3798179c84803233391585c68575aaa318ca61f4329b12b25d36eba3f1628a1c
```

**Player 2** (shows 1 finger, guesses total = 3):
```bash
cargo run --release --bin publish_bet -- \
  --round-id 100 --player-num 2 --h 1 --g 3 \
  --my-id     mtst1apekvcdafp5ekqrfhk2knm7yyyww4mlw \
  --player1-id mtst1az6qut6g4z8ucqqllwaups53ngcpdgks \
  --player2-id mtst1apekvcdafp5ekqrfhk2knm7yyyww4mlw \
  --house-id  mtst1ar9fdarcuvk0zqrnc44xdfmxp5yusalf \
  --faucet-id mtst1aqmat9m63ctdsgz6xcyzpuprpulwk9vg \
  --data-dir ./my-data
```

```
--p2-serial 1126125945eae6b4aa8daa379da233f60edfa11ffade09bb90303fd468eb5221
```

### Step 4 — House operator: settle the round

Once both players have sent their serial numbers and h/g values:

```bash
cargo run --release --bin settle_round -- \
  --round-id 100 \
  --house-id  mtst1ar9fdarcuvk0zqrnc44xdfmxp5yusalf \
  --faucet-id mtst1aqmat9m63ctdsgz6xcyzpuprpulwk9vg \
  --player1-id mtst1az6qut6g4z8ucqqllwaups53ngcpdgks \
  --p1-h 2 --p1-g 3 \
  --p1-serial 3798179c84803233391585c68575aaa318ca61f4329b12b25d36eba3f1628a1c \
  --player2-id mtst1apekvcdafp5ekqrfhk2knm7yyyww4mlw \
  --p2-h 1 --p2-g 3 \
  --p2-serial 1126125945eae6b4aa8daa379da233f60edfa11ffade09bb90303fd468eb5221 \
  --data-dir ./house-data
```

The house reconstructs both notes from the game params + serial numbers, submits a single settlement transaction, and the Miden VM verifies the outcome and emits payout notes.

```
Settlement complete!
Tx:      0x30bbafa28e73c064d4753b717cbffc869c66a52d6d9a1d17e42016ba20ccc1e2
Outcome: DRAW — 990000 base units each
```

---

## Live on Miden Testnet

The house account from the first demo is deployed at:

```
mtst1az7nmfpgwjpkcqpkm3fyuwwy7y9z43w3
```

Anyone can play against it by running `setup_player` and `publish_bet` — no code changes needed.

---

## Off-chain Coordination

The only things players need to share off-chain before the round:
- Agree on a unique `round-id`
- Share account IDs (player1, player2, house)

After publishing:
- Each player sends the house their `--pN-serial`, `h`, and `g`

The serial number is the only secret. Players should not share it until both have published their bets (to prevent the house from knowing both moves in advance and selectively settling). For a fully trustless version, use private notes with a commit-reveal scheme.

---

## CLI Reference

| Binary | Who runs it | When |
|--------|------------|------|
| `setup_player --role house` | House operator | Once, at deployment |
| `setup_player` | Each player | Once, per machine |
| `publish_bet` | Each player | Once per round |
| `settle_round` | House operator | Once per round, after both serials received |

**Common flags:**

| Flag | Description | Default |
|------|-------------|---------|
| `--bet-value` | Wager in base units (divisible by 50) | `1000000` |
| `--expiry-block` | Block after which player can request refund | `999999` |
| `--data-dir` | Directory for keystore + SQLite store | `..` |

---

## Build & Test

```bash
# Build contracts (house-account first — bet-note depends on its WIT)
cargo miden build --manifest-path contracts/house-account/Cargo.toml --release
cargo miden build --manifest-path contracts/bet-note/Cargo.toml --release

# Run MockChain integration tests (10 test cases)
# Note: run without a test-name filter — names are p1_wins, p2_wins, etc.
cargo test -p integration --release

# If you see stale compile errors, clean first:
cargo clean -p integration && cargo test -p integration --release

# Build all CLI binaries
cargo build --release -p integration
```

---

## Architecture

See [ARCHITECTURE_PLAN.md](./ARCHITECTURE_PLAN.md) for the full design: storage schema, 14 component methods, protocol requirements, payout math, and known limitations.
