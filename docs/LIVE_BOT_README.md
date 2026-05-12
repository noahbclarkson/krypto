# Live Bot — Quick Start Guide

**Purpose:** Run Turtle+Chandelier live on Binance testnet.
**Branch:** `v2-rewrite`
**Repo:** `https://github.com/noahbclarkson/krypto`

---

## Modes

| Flag | Mode | Orders | API Keys |
|------|------|--------|----------|
| (none) | DryRun | Simulated only | Not required |
| `--live` | Testnet | Real testnet orders | Required |
| `--live --prod` | Production | Real mainnet orders | Required + double-confirm |

---

## Step 1 — Create Binance Testnet Account

1. Go to **https://testnet.binancefuture.com/**
2. Log in → Dashboard → API Keys → Create New
3. Label it `krypto-testnet`
4. Enable "Enable Spot & Futures Trading"
5. **Save API Key + Secret** (secret shown only once)

### Faucet Testnet Funds

- BTCUSDT page → "Faucet" (top right) → request test BTC + USDT
- Minimum: 0.5 BTC, 5 ETH, 50 SOL, 5000 XRP, 50000 DOGE
- Repeat for each symbol you want to trade

---

## Step 2 — Set API Keys

```bash
export BINANCE_API_KEY="your_testnet_api_key"
export BINANCE_API_SECRET="your_testnet_api_secret"
```

Add to `~/.bashrc` to persist. **Never hardcode keys in code.**

---

## Step 3 — Verify Dry Run (No API Keys Needed)

```bash
cd ~/.openclaw/workspace-krypto/krypto
cargo run --example live_turtle_chandelier --profile sweep
```

Expected: paper backtest for 6 symbols, `DRY RUN` prefix on all output.
No API keys needed for this step.

---

## Step 4 — Dry Run With API Keys

```bash
BINANCE_API_KEY=xxx BINANCE_API_SECRET=yyy \
  cargo run --example live_turtle_chandelier --profile sweep
```

Look for `[DRY RUN]` prefix on all orders — confirms API keys work.

---

## Step 5 — Testnet Shadow Run (48h)

```bash
BINANCE_API_KEY=xxx BINANCE_API_SECRET=yyy \
  cargo run --example live_turtle_chandelier --profile sweep -- --live
```

- Real testnet orders placed (but `dry_run=true` in config — fills are simulated)
- Verifies: WebSocket streaming, signal firing, execution layer
- Run for 48h minimum. Look for `TURTLE ENTRY` log lines.

---

## Step 6 — Small Real Testnet Trades (5–10 trades)

Once shadow run shows correct signals:

```bash
# Edit examples/live_turtle_chandelier.rs:
# In the --live block: dry_run: true → dry_run: false
# Then re-run:
BINANCE_API_KEY=xxx BINANCE_API_SECRET=yyy \
  cargo run --example live_turtle_chandelier --profile sweep -- --live
```

Start with **BTCUSDT only** (1 symbol). Monitor for:
- Order placed on testnet
- Fill confirmation logged
- PnL tracking correct

---

## Production Safety Interlock

Production mode requires **two-step confirmation**:

```bash
# Step 1: ARM (dry run, logs what would happen)
cargo run --example live_turtle_chandelier -- --live --prod

# Step 2: CONFIRM (only runs if you re-run with --prod again)
# The bot will refuse to start on first --prod run.
# It requires a second explicit re-run.
```

**You must edit `examples/live_turtle_chandelier.rs`** to remove the production guard before step 2.

---

## Expected Output Per Mode

### DryRun (no flags)
```
DRY RUN — no orders placed
[paper] Turtle+Chandelier backtest complete
```

### Testnet (--live)
```
[DRY RUN] BUY BTCUSDT @ 94321.50 — on testnet
WebSocket bar: BTCUSDT 2026-04-19 00:00 ...
TURTLE ENTRY detected on BTCUSDT
```

### Production (--live --prod, first run)
```
ERROR: Production mode requires explicit confirmation.
Re-run with --prod flag to arm.
```

---

## Production Parameters (Frozen — per HALL_OF_FAME.md 2026-04-30)

```
EP                       = 21     (Turtle entry lookback)
TURTLE_ATR_PERIOD       = 24     (Turtle ATR period)
TURTLE_ATR_MULT         = 2.00   (Turtle ATR multiplier)
CHAND_PERIOD            = 7      (Chandelier ATR period — stored but UNUSED by live bot.rs; live exit is Turtle ATR only)
CHAND_MULT              = 2.30   (Chandelier multiplier)
HOLD_MAX                = 15     (max bars held — updated 2026-05-10 from 12, T88 exact-live confirmed)
POSITION_CAP            = 3      (max concurrent positions)
FRESHNESS_COOLDOWN      = 0      (no freshness filter)
ATR_ENTRY_MULT          = 0.00   (no ATR entry filter)
VOL_LOOKBACK            = 8      (dollar-volume lookback)

# Regime / entry filter (live bot only)
ATR_RANK_THRESHOLD      = 5      (block if BTC ATR in bottom 5% of 252-bar history)
REGIME_ATR_PERIOD       = 17     (BTC ATR period for rank calculation — updated 2026-05-04 from 12)
REGIME_LOOKBACK         = 41     (lookback for ATR percentile rank — updated 2026-05-08 from 42)

# Position caps
MAX_SOL_POSITION        = $50K notional
```

---

## Known Live Risks

| Risk | Mitigation |
|------|-----------|
| SOL slippage > model | Cap SOL at $50K notional |
| Maker fill ~63% (vs 70% assumed) | Place entry limit orders at bar close, Post Only |
| No regime defense | Strategy whipsaws in extended chop (2026 YTD: -22.7%) |
| No live stop-loss order | Chandelier exit on next bar open — gap risk exists |

---

## Emergency Stop

To stop the bot immediately:
```
Ctrl+C
```

To prevent accidental orders after restart, unset API keys:
```bash
unset BINANCE_API_KEY BINANCE_API_SECRET
```

---

*Last updated: 2026-04-30. Parameters frozen per MEMORY.md.*
