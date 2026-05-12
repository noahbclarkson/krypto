# Deployment Checklist — Binance Testnet

**Status: READY. Waiting on Noah for API keys.**

---

## What We Have

- **Validated strategy:** Turtle breakout (EP=21) + Turtle ATR(24, 2.0) trailing stop
- **Live bot:** `src/live/bot.rs` — WebSocket market feed + live order execution
- **Entrypoint:** `examples/live_turtle_chandelier.rs`
- **Paper validation:** ✅ 397 trades, all symbols positive, 49.8% win rate, 158% avg return
- **Walk-forward:** 100% Base5 pass (6/6), 83% global pass (44/54)
- **All params frozen** — no more hyperopts needed

---

## What We Need

**Noah:** Binance Futures testnet API key + secret.

1. Go to https://testnet.binancefuture.com/
2. Log in → API Key management → Create testnet key
3. Give the key to Kira (or set in environment)

---

## How to Run (Testnet)

```bash
cd ~/.openclaw/workspace-krypto/krypto

# Dry run (paper, no real orders) — always works, no keys needed:
cargo run --example live_turtle_chandelier --profile sweep

# LIVE testnet (real orders on testnet.binancefuture.com):
BINANCE_API_KEY=xxx BINANCE_API_SECRET=yyy \
  cargo run --example live_turtle_chandelier --profile sweep -- --live
```

**Safety interlocks:**
- `dry_run = true` (default): no real orders placed
- `--live` flag: enables testnet orders
- `--prod` flag: requires re-confirmation + `use_testnet=false` — prevents accidental mainnet
- API credentials checked at startup — bot refuses to start without them in non-dry mode

---

## What the Bot Does

1. Connects to Binance WebSocket stream (`wss://testnet.binancefuture.com/ws`)
2. Fetches 60+ warmup daily bars for each symbol via REST
3. Monitors for Turtle breakout: `close > max(close[EP bars])`
4. On breakout: places **limit buy order** at bar close (favoring maker fill)
5. Tracks position with **Turtle ATR trailing stop** (sole exit)
6. Exit: when `low <= lowest_low - ATR(24) * 2.0` OR `bars_held >= 15`
7. Logs all fills to `logs/slippage_YYYY-MM-DD.csv`

---

## Live Bot Params (from `src/live/config.rs`)

```
EP               = 21
ATR_PERIOD       = 24
ATR_MULT         = 2.0
ATR_ENTRY_MULT   = 0.00  (no entry filter)
HOLD_MAX         = 15
POSITION_CAP     = 3
FRESHNESS_COOLDOWN = 0
REGIME_ATR_PERIOD = 17
REGIME_LOOKBACK   = 41
ATR_RANK_THRESHOLD = 5.0
```

**Note:** Chandelier params in `LiveConfig` (CHAND_P=7, CHAND_M=2.30) are stored but **not used by bot.rs** — the live bot uses Turtle-only exit as validated 2026-04-27.

---

## Symbols

`BTCUSDT, ETHUSDT, SOLUSDT, XRPUSDT, DOGEUSDT` (Base5 — all validated)

---

## Expected Live Behavior

- Maker-fill rate ~65-70% (from microstructure analysis)
- Realistic Sharpe degradation: 22-33% vs gross backtest
- Fee-adjusted walk-forward Sharpe: ~3.1–3.7
- Max concurrent positions: 3 (CAP=3)
- Position size: `initial_capital / 3` per symbol

---

## Monitoring

```bash
# Check logs
tail -f logs/slippage_$(date +%Y-%m-%d).csv

# Check bot state (if state endpoint is exposed)
# Currently: local logging only
```

---

## If Something Goes Wrong

1. **Bot refuses to start**: Check API keys are valid + have testnet futures permissions
2. **No trades for >30 days**: Verify WebSocket is receiving data — check `tradingview.com` for candle updates
3. **All positions negative**: 2026 is a bear/chop regime — expected. Strategy should still outperform buy-hold via trailing stops
4. **WebSocket disconnects**: Binance has rate limits — bot reconnects automatically

---

## After 30 Days Testnet

- Review `logs/slippage_*.csv` for real execution quality
- Compare live Sharpe vs backtest expectation (~3.1–3.7 fee-adj)
- If live Sharpe > 2.0: consider mainnet paper trading
- If live Sharpe < 1.0: surface to Kira for investigation
