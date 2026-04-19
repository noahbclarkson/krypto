# PLAN.md — Krypto Live Testnet Priority

**Manager directive (2026-04-18):** Stop all historical hyperopt/doc loops. Research closed. Next useful outputs only:
1. Minimal Binance testnet readiness checklist for Noah
2. Exact env/API setup
3. One dry-run/testnet launch plan
4. Any code changes strictly required for safe first live shadow run

---

## ✅ COMPLETED (2026-04-19)

### T1: HALL_OF_FAME.md Freshness Cooldown ✅
- Already correct. HALL_OF_FAME.md line 25: `FRESHNESS_COOLDOWN = 0`. bot.rs:21: `cd=0`. No discrepancy.

### T2: daily_progress.csv Archive ✅
- Archived to `daily_progress_LEGACY.csv`. Stub file with LEGACY warning created.

### T3: Progress Equity CSV Integrity ✅ (06:03 UTC)
- Verified: Turtle 673.5x (was 672.7x — <1% data-refresh variance). Equity internally consistent.

### Walk-Forward Harness Sync ✅ (09:05 UTC)
- P=15/M=1.50 fully validated: 43/54 (79.6%) global, 6/6 Base5.
- Walk-forward harness now synced.

### Entry Filter Sweep ✅ (morning session)
- ATR entry filter: REJECTED (mult=0.0 wins). Volume confirmation: REJECTED.
- No production code changes needed.

### Examples Hygiene ✅ (12:18 UTC)
- 33 stale examples archived to `examples/GRAVEYARD/`
- 320 active examples remaining (was 351)
- Build: clean ✅

---

## 📋 BINANCE TESTNET READINESS CHECKLIST — For Noah

### Step 1: Create Binance Testnet Account (5 minutes)

1. Go to **https://testnet.binancefuture.com/**
2. Log in with your GitHub or email account
3. Navigate to **Dashboard → API Keys → Create New**
4. Label it `krypto-testnet` (or any name you prefer)
5. **Save the API Key and Secret** — you will only see the secret once
6. Enable "Enable Spot & Futures Trading" if not already enabled
7. No IP restriction needed for first test

**⚠️ Testnet funds are free.** Request testnet USDT from:
- `https://testnet.binancefuture.com/en/futures/BTCUSDT` → click "Faucet" (top right)
- Request at least 10,000 USDT test funds
- Each symbol also needs test tokens (BTC, ETH, SOL, etc.) — faucet those too

### Step 2: Verify Dry Run Works (before any real testnet orders)

```bash
# Verify the bot compiles and runs paper/dry-run mode first
cd ~/.openclaw/workspace-krypto/krypto

# Dry run — no API keys needed, simulates orders only
cargo run --example live_turtle_chandelier --profile sweep
```

Expected output: paper backtest results for 5 symbols, no API errors.

### Step 3: Set Environment Variables

```bash
# Add to your shell profile (~/.bashrc or ~/.zshrc) or run inline:
export BINANCE_API_KEY="your_testnet_api_key_here"
export BINANCE_API_SECRET="your_testnet_secret_here"
```

**Never put API keys in code or git.**

### Step 4: Verify Testnet Connection (dry-run, then real)

```bash
# Dry run with testnet config (simulated orders, no real trades)
BINANCE_API_KEY=xxx BINANCE_API_SECRET=yyy \
  cargo run --example live_turtle_chandelier --profile sweep

# Real testnet orders (shadow mode — bot logs signals but doesn't trade)
BINANCE_API_KEY=xxx BINANCE_API_SECRET=yyy \
  cargo run --example live_turtle_chandelier --profile sweep -- --live
```

Look for `[DRY RUN] BUY BTCUSDT ... on testnet` in the output. This confirms API keys work.

### Step 5: Symbols to Fund for Testnet

| Symbol | Why needed | Approx test funds |
|--------|-----------|-----------------|
| BTCUSDT | Primary trend | 0.5 BTC |
| ETHUSDT | Primary trend | 5 ETH |
| SOLUSDT | High slippage risk | 50 SOL |
| XRPUSDT | Secondary trend | 5000 XRP |
| DOGEUSDT | High vol trend | 50000 DOGE |

---

## 🧪 DRY-RUN / TESTNET LAUNCH PLAN

### Phase 0 — Verify (Do First)

```bash
# 1. Compile check
cargo build --example live_turtle_chandelier --profile sweep

# 2. Paper mode (no API keys needed)
cargo run --example live_turtle_chandelier --profile sweep

# 3. Dry-run with testnet API keys (simulates, no real orders)
BINANCE_API_KEY=xxx BINANCE_API_SECRET=yyy \
  cargo run --example live_turtle_chandelier --profile sweep
```
Expected: `DRY RUN` prefix on all orders, no real order IDs returned.

### Phase 1 — Shadow Run (Live signals, observe only)

```bash
# Bot starts, watches WebSocket, logs all signals but DOES NOT TRADE
# The --live flag activates real executor but dry_run=true by default
# from_env() + no --prod flag = testnet, dry_run=true (safe default)

BINANCE_API_KEY=xxx BINANCE_API_SECRET=yyy \
cargo run --example live_turtle_chandelier --profile sweep -- --live
```

Look for:
- `[DRY RUN] BUY BTCUSDT ... on testnet` = signal detected, simulated order placed ✅
- WebSocket bar updates streaming in
- `TURTLE ENTRY` log lines when breakout detected

**Duration:** Run for 1-2 days. Verify signals fire correctly on live data.

### Phase 2 — Small Real Testnet Trade (if shadow run looks correct)

Once shadow run shows correct signals:

```bash
# Edit examples/live_turtle_chandelier.rs to set dry_run=false for testnet
# Change: dry_run: true → dry_run: false  (in the --live block)
# Run again
```

Start with **1 symbol only** (BTCUSDT). Monitor for:
- Order placed on testnet exchange
- Fill confirmation logged
- PnL tracking correct

**Duration:** 5-10 trades to validate execution quality.

### Phase 3 — Full 5-Symbol Testnet Run

After Phase 2 validates:
- Maker fill rate ~60-70%
- Slippage within model
- Signal timing correct
- PnL tracking correct

Run full universe (BTC, ETH, SOL, XRP, DOGE) on testnet for **30 days**.

---

## 🔒 REQUIRED CODE CHANGES — Safe First Live Shadow Run

**Minimal required change (one file, ~15 lines)**

**`examples/live_turtle_chandelier.rs`** — modify the `--live` block to add a prominent warning:

```rust
// Add prominent warning in the --live block:
println!();
println!("{}", "╔════════════════════════════════════════════════════════════╗".bold().red());
println!("{}", "║  ⚠️  LIVE TESTNET MODE — Real testnet orders will be placed  ║".bold().red());
println!("{}", "║  Funds are testnet only — no real value lost               ║".bold().red());
println!("{}", "╚════════════════════════════════════════════════════════════╝".bold().red());
println!();
```

---

## ✅ CODE REVIEW: What's Already Safe

| Component | Status | Notes |
|-----------|--------|-------|
| Signal logic (Turtle+Chandelier) | ✅ Verified | Matches walk-forward harness exactly |
| Freshness filter (cd=0) | ✅ In bot.rs | Line 21: `const FRESHNESS_COOLDOWN: usize = 0;` |
| Dual exit (Chandelier + Turtle ATR) | ✅ In bot.rs | Lines 196-210: both stops checked, tighter wins |
| Position cap (CAP=3) | ✅ Enforced | `check_dual_exit` only checks if in position; `process_bar` enforces cap |
| Dry run default | ✅ Safe | `dry_run: true` is default in `LiveConfig::default()` |
| Testnet default | ✅ Safe | `use_testnet: true` is default — won't hit mainnet accidentally |
| API key env vars | ✅ Secure | Read from `BINANCE_API_KEY` / `BINANCE_API_SECRET`, never hardcoded |
| Exit logging | ✅ Complete | All exits logged with symbol, price, PnL%, bars held |
| Freshness tracking | ✅ In bot.rs | `last_exit_bar` HashMap tracks bar of last exit per symbol |

---

## ⚠️ KNOWN LIVE TRADING RISKS (document for Noah)

1. **SOL slippage exceeds model** at >$50K notional. Cap SOL position at $50K or reduce to 0.5x size.
2. **Maker fill ~63%** (actual) vs 70% (model). Slightly conservative — ok.
3. **No regime defense.** Strategy will underperform badly in choppy 2026-type markets.
4. **2026 YTD -22.7%** — if live markets stay choppy, live Sharpe could be negative.
5. **No live stop-loss order** — exits are signaled on next bar open. Gap risk exists.

---

## 🚫 STOP DOING

- No more walk-forward parameter optimization
- No more hyperopt passes
- No more strategy research
- No more Monte Carlo on historical data
- No more documentation loops

**The only question live testnet can answer that history cannot:**
- Does the signal fire correctly on current (live) market data?
- Does the execution layer work (order placement, fill confirmation, PnL tracking)?
- Is the maker fill rate in the expected range?

All other questions have been answered. Ship it.

---

## 📅 TOP PRIORITY

1. **Noah creates testnet account + faucets funds** (5 min, blocks on him)
2. **Verify dry-run mode works** (10 min, no API keys needed)
3. **Run shadow mode 48h** — verify live signals fire correctly
4. **Run small real testnet trades** (5-10 trades, 1 symbol)
5. **30-day full universe testnet run** → measure live Sharpe vs backtest Sharpe

Once 30-day live data is in: compare actual Sharpe vs expected ~1.0-1.3. If > 0.5 → proceed. If < 0.5 → diagnose before any production.

---

*Last updated: 2026-04-19 12:18 UTC — T1/T2/T3 complete, examples hygiene complete. Research closed. Live testnet only.*
