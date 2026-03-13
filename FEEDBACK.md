# Agentic Tooling Feedback — Alla Morra

## What worked well

**Skills were accurate and saved significant research time.**
`rust-sdk-testing-patterns` correctly described the `MockChainBuilder` / `MockChain` split and the `build_project_in_dir` pattern. Without it, discovering that notes must be added to the *builder* before `build()` — not to the chain after — would have required reading Miden source directly. The pitfall about asset conservation was well-documented in `rust-sdk-pitfalls` and matched the actual runtime failure we hit.

**Plan Mode produced a complete architecture up-front.**
The plan captured the storage key schema, the 14 component methods, the note-input layout, and the full settlement algorithm before a single line was written. This meant that when we hit API errors, the plan was right about what we wanted to do — only the exact API names were wrong. Having the architecture locked before implementation prevented scope creep.

**Sub-agents for exploration worked cleanly.**
Using an Explore sub-agent to locate the `MockChain` API without filling the main context was exactly the right tradeoff. The agent returned precise file paths and method signatures and was done in one round.

**The `build-contracts.sh` hook gave instant feedback.**
Every contract edit triggered a build automatically. This caught brace/syntax errors immediately rather than at test time.

---

## What was missing, confusing, or incorrect

**`rust-sdk-pitfalls` should document the asset-transfer requirement.**
The biggest non-obvious runtime failure was: *note assets are NOT automatically moved to the consuming account's vault*. Every note script that touches assets must explicitly call `native_account::add_asset` — and that call must go through an account component method because the kernel requires native-account execution context. This is easily a 30-minute debugging spiral if you don't know it. It's not in `rust-sdk-pitfalls`, `rust-sdk-patterns`, or `rust-sdk-testing-patterns`.

**The plan's `NoteType::Private` constant was wrong.**
The plan said `NoteType::Private` but the actual SDK uses `NoteType::from(felt!(2))`. The plan was confident enough about this that it wasn't flagged as "unverified" — it should have been, or the skill should document the correct form.

**`Asset::new` vs `Asset::from` ambiguity.**
The plan noted this as an open question; it turned out to be `Asset::new(Word)`. A note in the skill with the correct constructor would have saved a build iteration.

**`AccountId.prefix()` returns `AccountIdPrefix`, not `Felt`.**
The plan assumed `.prefix().as_felt()` was valid. It isn't — you need `.prefix().into()`. This is a subtle type mismatch that causes a confusing E0599 error. The `rust-sdk-testing-patterns` skill should document `id_felts` as a helper pattern.

**`Digest::from_word` needed an explicit import.**
`use miden::*` does not re-export `Digest` in a way that makes `Digest::from_word` available without explicitly importing it. This caused a single-line fix but it wasn't predictable.

**MockChain block numbering is not documented.**
Genesis = block 0. `tx::get_block_number()` returns the *reference block* for the transaction, which is 0 at genesis. `prove_next_block()` advances it to 1. The `expiry_recall` test required understanding this to set up the right `expiry_block` value. This belongs in `rust-sdk-testing-patterns`.

**The `build-contracts.sh` hook fires for the wrong project directory.**
When working in a sibling repo (`alla-morra/`), the hook in `agentic-template/project-template/` fires on every edit and produces a blocking error because the contract paths don't match. The hook should either be scoped to `project-template/` only, or should exit silently if no matching contracts are found.

---

## Suggested improvements to skills, hooks, or documentation

**Add to `rust-sdk-pitfalls`:**
- Asset transfer rule: note scripts must call `native_account::add_asset` via an account component method; the kernel will reject direct calls from note context.
- Block numbering: genesis = 0, `prove_next_block()` advances by 1, `tx::get_block_number()` returns the reference block (not current head).
- `AccountId` decomposition: `id.suffix()` → `Felt`; `id.prefix()` → `AccountIdPrefix`, convert with `.into()`.

**Add to `rust-sdk-patterns`:**
- Correct `NoteType` construction: `NoteType::from(felt!(2))` for private.
- Correct `Asset` constructor: `Asset::new(Word::from([amount, felt!(0), faucet_suffix, faucet_prefix]))`.
- The "receive_asset via component" pattern: when a note needs to transfer its own asset to the consuming account, it must call a component method that calls `native_account::add_asset`.

**Add to `rust-sdk-testing-patterns`:**
- `id_felts(id: AccountId) -> (Felt, Felt)` helper using `.suffix()` and `.prefix().into()`.
- MockChain block numbering and the `prove_next_block()` pattern for expiry tests.
- `add_existing_basic_faucet(auth, symbol, max_supply, total_issuance: Option<u64>)` signature, and that `total_issuance: Some(x)` is required for the VM to accept issued assets in notes.

**Fix `build-contracts.sh` hook:**
Scope it to only fire when the edited file is inside `project-template/contracts/`. Add a guard at the top:
```bash
[[ "$EDITED_FILE" == *project-template/contracts/* ]] || exit 0
```

**New pattern to capture: house-mediated expiry recall.**
In Miden, a note script cannot transfer assets back to a player (non-house) account because the player account won't have the house component installed. The correct pattern for expiry/refund is: house always consumes the note and emits a new P2ID-style output note to the player. This "house as intermediary" pattern should be documented as a canonical design for game/escrow contracts.

---

## Patterns that should become new skills

**`miden-escrow-patterns` skill** — covers contracts where a house/escrow account holds assets between two parties:
- Asset intake via `receive_asset` component method
- Payout note creation with deterministic serial numbers
- Settlement flag to prevent double-spend
- House-mediated expiry recall
- Participant commitment (XOR or RPO hash) for cross-note validation

**`miden-note-lifecycle` skill** — walks through a note's full lifecycle in testing:
- Creating notes with `create_testing_note_from_package`
- Adding to `MockChainBuilder` before `build()`
- Executing a transaction that consumes the note
- Verifying output notes and asset amounts
- Advancing blocks and testing time-based logic
