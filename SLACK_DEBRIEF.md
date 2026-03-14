Hey team 👋 — sharing a debrief from testing our agentic dev workflow end-to-end on a real project.

**What I built:** Morra on Miden. Morra is a 2-player finger game (show 0–3 fingers, guess the combined total — if only you guess the exact total, you win; if both or neither guess correctly, it's a draw). I implemented it as on-chain smart contracts on Miden, CLI-only, no frontend. Two players can each run a command from their own machine to submit their move; a house operator settles the round on-chain. All game logic is verified by the VM — the house can't cheat.

**What got shipped:** 2 smart contracts, 10 integration tests, 4 CLI binaries, live game played on testnet. Repo: https://github.com/guelowrd/alla-morra

**Verdict:** Plan Mode + domain skills meaningfully accelerated the contract design phase. The gaps were in skill coverage (key Miden SDK behaviours missing from the pitfalls docs) and verification discipline (agent trusted a stale test cache and reported false passes). Both fixable.

Full structured feedback + architecture doc attached — worth a read if you're building on top of this workflow or maintaining the skills.
