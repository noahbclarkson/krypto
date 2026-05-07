# Krypto Deployment Runbook

**Purpose:** Step-by-step checklist for deploying krypto live on Binance testnet → production.
**Status:** DEPLOYMENT-READY — awaiting Noah's API keys.
**Last updated:** 2026-05-07

---

## Prerequisites

- [ ] Binance testnet account (`https://testnet.binancefuture.com/`)
- [ ] API key + secret for testnet
- [ ] Testnet faucet funds: ≥0.5 BTC, 5 ETH, 50 SOL, 5000 XRP, 50000 DOGE
- [ ] `cargo build --profile sweep` succeeds (already verified 2026-05-07 ✅)

---

## Step 1 — Dry Run (No API Keys Required)

```bash
cd ~/.openclaw/workspace-krypto/krypto
cargo run --example live_turtle_chandelier --profile sweep
```

Expected output: `DRY RUN` prefix on all orders. Paper backtest for Base5 symbols.
**Success criteria:** Completes without error, equity curve plausible (2-3x range).

---

## Step 2 — Dry Run With API Keys (Verify Connectivity)

```bash
export BINANCE_API_KEY="your_testnet_key"
export BINANCE_API_SECRET="your_testnet_secret"

cargo run --example live_turtle_chandelier --profile sweep
# Look for: [DRY RUN] prefix confirms API keys authenticate correctly
```

**Success criteria:** Logs show API auth succeeds (no 403/401 errors). Orders show `[DRY RUN]` — no real testnet funds spent.

---

## Step 3 — Testnet Shadow Run (48h Minimum)

```bash
BINANCE_API_KEY=xxx BINANCE_API_SECRET=yyy \
  cargo run --example live_turtle_chandelier --profile sweep -- --live
```

- Real testnet orders placed with `dry_run=true` in config — fills are simulated
- Verifies: WebSocket streaming, signal firing, execution layer
- Minimum runtime: 48 hours to cover at least one regime transition
- Look for: `TURTLE ENTRY` log lines confirming live signal processing

**Success criteria:** No crashes, no order rejects, equity curve tracks paper backtest closely.

---

## Step 4 — Testnet Live Run (1 Week)

```bash
# Edit src/live/config.rs: dry_run = false (testnet mode)
BINANCE_API_KEY=xxx BINANCE_API_SECRET=yyy \
  cargo run --example live_turtle_chandelier --profile sweep -- --live
```

- Real testnet orders placed and filled at market
- Monitor maker/taker fill rate (target: ≥40% maker)
- Log all fills for post-hoc analysis

**Success criteria:** 
- Maker fill rate ≥35% (target 40-70%)
- Fee-adjusted Sharpe ≥0.8 (vs backtest 0.98 — allow 20% degradation)
- No more than 1 anomalous fill per 100 trades

---

## Step 5 — Production Readiness Checklist

Before going to production with real capital:

- [ ] Testnet live run completed ≥7 days
- [ ] Maker/taker fill rate verified ≥35%
- [ ] Fee-adjusted equity ≥0.8× backtest baseline (2.31x minimum)
- [ ] No API errors, WebSocket reconnection loops, or order rejects
- [ ] Daily equity curve tracked backtest within 1 StdDev
- [ ] MaxDD ≤ 40% (backtest was 29% — allow 10pp tolerance for live)
- [ ] Win rate ≥42% (backtest was 48%)

---

## Escalation Criteria — When to Halt

| Trigger | Threshold | Action |
|---------|-----------|--------|
| Maker fill rate | <20% for 3 consecutive days | Pause — fees too high |
| Equity drawdown | >40% from peak | Pause — validate live vs backtest |
| Sharpe (7d rolling) | <0.3 annualized | Pause — edge may have degraded |
| Win rate | <38% over 50 trades | Pause — check signal logic |
| API errors | >10/day | Pause — connectivity issue |

**Rule:** When in doubt, pause. Paper results are upper bounds.

---

## Strategy Summary (From HALL_OF_FAME.md)

```
Symbol universe: BTC, ETH, SOL, XRP, DOGE, ADA (Base5+ADA)
Entry:           Turtle breakout — close > max(close, EP=21)
Entry gate:      ATR rank ≥5th pctile of 252-bar history (noise filter)
Exit:            Turtle ATR trailing stop — highest_high − 2.0×ATR(24)
Position cap:    3 concurrent positions
Hedge:           BTC ATR38 > 45th pctile → position size × 0.40
Max hold:        12 bars (structurally rarely binding — ATR stop fires first)
Fee model:       4 bps/side taker (backtest), 0 bps maker (realistic)
```

---

## Risk Parameters

```text
Max position size:  1/n per position (n = open positions, max 3)
Max drawdown halt:  40% of peak equity
Max daily trades:   no hard limit (signal-driven)
Min maker fill:     35% (below this, fees erode edge)
```

---

## Monitoring Commands

```bash
# Check running process
ps aux | grep live_turtle

# View live logs (if running as service)
journalctl -u krypto -f

# Check disk space before starting
df -h ~/.openclaw/workspace-krypto/krypto/
```

---

## Rollback Procedure

1. `Ctrl+C` or `kill` the running process
2. Set `dry_run=true` in config
3. Review `snapshots/live_bot_exact_equity.csv` for equity at halt
4. Do not restart until root cause identified

---

## Emergency Contacts

- **Arc (orchestrator):** via `sessions_send agent:main:main` for blockers
- **Noah:** Discord #krypto for approval to go to production

---
