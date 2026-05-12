# PLAN.md — Krypto Research and Execution Plan

**Updated: 2026-05-12 16:05 UTC — Critique Session #13**

---

## Current State

**Research loop: CLOSED. Execution loop: BLOCKED 6+ WEEKS.**

- **Suspension animation confirmed:** 5+ consecutive docs-only sessions.
- **T99 pre-2021 validation = bull market validation.** Pre-2021 dominated by 2017-2018 and 2020-2021 bulls. 100% pass rate is not cross-regime proof.
- **2026 YTD underperformance is structural non-stationarity**, not "accepted limitation."
- **All Turtle-family params: FROZEN.**
- **Exact-live: 2.76x / Sharpe 1.02 / MaxDD 22.3% / 286 trades / 1,801 days.**

---

## Critical Open Issues

### 🚨 API Key Blocker — 6+ Weeks Without Escalation (Anti-Spin #12 TRIGGERED)
- "Main blocker: Binance testnet credentials" has been in PLAN.md since ~2026-04-10
- **Anti-spin rule #12:** "If blocked on external dependency for 5+ weeks, need an explicit plan."
- **ESCALATION REQUIRED THIS SESSION TO ARC.**
- Options to present: (a) keys available → start testnet, (b) keys absent → suspend cron sessions, (c) explicit decision to continue docs-only

### 🚨 2026 YTD: Structural Non-Stationarity
- ATR_RANK T=5 gate: in low-vol regimes (current 2026), gate doesn't fire → unfiltered exposure.
- Top-10 = 91% of log return. Convex tail fragility is the single biggest structural risk.
- **No fix path without destroying convex tail.** Understanding only.

### 🚨 Suspension Animation — Anti-Spin #11 Triggered
- 5+ consecutive docs-only commits documented.
- Every session without live data produces no new validated information.

---

## Top-3 Execution Tasks (Updated 2026-05-12)

### Task 1: Top-Winner Mechanism Decomposition — BUILD
**What:** `examples/t104_top_winner_decomposition.rs` documents WHY the top-10 trades are so large.

**For each top-10 trade, document:**
- Entry date, symbol, position size
- BTC 21d return at entry (vol regime)
- BTC ATR percentile at entry (gate active/inactive?)
- Holding bars before exit
- Exit type (Turtle ATR stop vs profitable exit)
- BTC regime label at entry

**Deliverable:** `snapshots/t104_top_winner_mechanism.md` — structural explanation of the 91% convex tail.

**Anti-spin:** NOT a filter task. NOT a parameter sweep. Understanding only.

### Task 2: Escalate API Key Blocker to Arc — ESCALATE NOW
**Message to Arc:**
```
Project: krypto v2-rewrite
Research loop: closed. All Turtle params settled.
Execution loop: blocked 6+ weeks on Binance testnet API keys.
Current state: 5+ consecutive docs-only sessions.
Request: (a) keys available → start live testnet immediately
         (b) keys absent → suspend cron sessions until available
         (c) explicit decision to continue in docs-only mode
```

### Task 3: Production Universe Document — WRITE
**What:** `krypto/docs/production_universe.md` formally documents:
- Base5 (BTC/ETH/SOL/XRP/DOGE/ADA) is the production universe
- Why these 6 symbols (DV rank, data quality, trending behaviour)
- Why LTC/EOS/BCH excluded (survivorship bias — these failed validation)
- UNI/AVAX/MATIC out-of-scope (generalization failure documented in T80)
- UNI 1/6 pass, AVAX 4/6 pass, MATIC history ends 2024-09 (missing 2025-2026 regime)
- This is honest scope definition, not strategy weakness

---

## Project Tracks

### Track A — Trust the Lab

**Status: COMPLETE. No further validation without live data.**

Regression guardrail after any `src/live/bot.rs` change:
1. `cargo build`
2. `cargo run --example live_bot_historical_replay --profile sweep`
3. `cargo run --example live_bot_exact_equity --profile sweep`
Expected: 286 trades, 2.76x / Sharpe 1.02 / MaxDD 22.3%

### Track B — Stress the Current Leader

**Status: COMPLETE. No further Turtle-family stress tests.**

Known structural risks (documented):
- **2026 YTD: -3.2%, Sharpe -1.17. Non-stationary ATR_RANK T=5 gate.**
- **Top-10 concentration: 91% of log return. Convex tail fragility.**
- **UNI generalization: 1/6 pass. Universe-sensitive.**
- **Base5 survival: BTC/ETH/SOL/XRP/DOGE/ADA only.**

### Track C — Broaden Edge Discovery

**Status: SUSPENDED until live testnet.**

Valid work:
1. Top-winner decomposition (understanding, not filtering)
2. Order-flow dynamic position sizing (different from entry filter)
3. No Turtle-family param sweeps

---

## Honest Production Statement

**What we have:** Turtle ATR trend-following on daily crypto. 2.76x over 1,801 days. Sharpe 1.02. MaxDD 22.3%. 286 trades. Pre-2021 validation passed (but is bull-market-dominated). Historical replay validated. Fee impact confirmed negligible.

**What we DON'T have:** Live execution feedback (6+ weeks blocked). Confirmed live execution path.

**What we know is broken:** 2026 YTD (-3.2%, Sharpe -1.17). Top-10 91% tail dependency. UNI 1/6 generalization failure.

**What the next session MUST decide:** Live testnet OR explicit suspension.

---

## Anti-Spin Rules (Active — Updated 2026-05-12)

1. No Turtle-family parameter sweeps unless a new mechanism is proposed.
2. No "audit" tasks — write the test or close the issue.
3. Every task must have an execute-or-close decision.
4. **621x / 176.79x numbers are research diagnostics, NOT production performance.**
5. Daily account Sharpe only on equity charts. Per-window walk-forward Sharpe not comparable.
6. Report fee-adjusted Sharpe as a range, not a point estimate.
7. **No candidate is production-valid until exact-live replay verification.**
8. Top-10 = 91% of log return. Any new filter must preserve the convex tail.
9. **Chandelier either fires or is removed. Non-binding exits are docs errors.**
10. **Universe selection is survivorship bias.** Always disclose which assets.
11. **Suspension animation is a real failure mode.** 5+ consecutive docs-only commits = escalate.
12. **If blocked on external dependency for 5+ weeks, need an explicit plan.**
13. **Maker-fill uncertainty is confirmed [1.02-1.04] — not dominant.**
14. **Progress harness 621x ≠ live bot 2.76x.** Do not conflate.
15. **2026 YTD underperformance: silent failure.** Not "accepted limitation" — an active documented structural failure.
16. ~~M1 Discord~~ — CLOSED. M1 is console-only, not a Discord tool.
17. **API key blocker: 6+ weeks. Escalate to Arc per rule #12.**
18. **Top-winner mechanism decomposition is the only path to understanding 91% tail.**
19. **Research loop is closed. No further Turtle-family validation without live data.**
20. **2026 YTD: no fix path without destroying convex tail. Document mechanism, accept uncertainty.**
21. **Pre-2021 validation = bull market validation. 100% pass ≠ cross-regime proof.**
22. **T99 pre-2021 validation dominated by 2017-2018 and 2020-2021 crypto bulls.**
