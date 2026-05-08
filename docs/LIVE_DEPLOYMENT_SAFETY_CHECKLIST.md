# Live Deployment Safety Checklist

**Purpose:** Pre-launch verification and live kill-switch criteria for krypto live trading.
**Replaces:** Implicit safety logic spread across runbook and memory notes.
**Last updated:** 2026-05-08

---

## Pre-Launch Verification (Run Before Any Live Execution)

### API & Connectivity
- [ ] API key + secret confirmed valid: testnet ping succeeds
- [ ] WebSocket stream subscribed: `!userDataStream` start confirms
- [ ] Testnet faucet funds verified: ≥0.5 BTC, 5 ETH, 50 SOL, 5000 XRP, 50000 DOGE
- [ ] No API rate-limit flags in logs from prior dry runs

### Code Integrity
- [ ] `src/live/config.rs` — dry_run = true for testnet, = false for production only
- [ ] `HEDGE_SIZE_MULT = 0.25` confirmed (last validated T83)
- [ ] `HOLD_MAX = 15` confirmed
- [ ] `POSITION_CAP = 3` confirmed
- [ ] No stale overrides in `live_turtle_chandelier.rs` example

### Backtest Sanity Check
- [ ] Exact-live equity ≥2.5x (current: 2.76x ✅)
- [ ] Daily Sharpe ≥0.8 (current: 1.02 ✅)
- [ ] MaxDD ≤ 30% (current: 22.3% ✅)
- [ ] Trades ≥ 200 (current: 286 ✅)

---

## Kill-Switch Criteria

### Hard Stop — Immediate Halt
| Trigger | Threshold | Action |
|---------|-----------|--------|
| Account drawdown | >35% from peak | **STOP immediately.** Do not restart until root cause identified. |
| Daily maker fill rate | <20% for any single day | Pause — fee model invalid, edge likely negative |
| Consecutive losing days | >15 trading days with no profitable day | Pause — regime shift or signal failure |
| Equity halving | Equity drops to ≤50% of peak | **STOP immediately.** Account-level risk management. |

### Soft Pause — Investigate Before Continuing
| Trigger | Threshold | Action |
|---------|-----------|--------|
| Maker fill rate | <30% for 5 consecutive days | Alert. Lower position size or pause. |
| 30-day rolling Sharpe | <0.0 (negative) | Pause — check live vs backtest drift |
| Win rate | <38% over 50 trades | Pause — signal logic may be malfunctioning |
| API errors | >5/day | Pause — connectivity issue |
| Slippage per fill | >15 bps vs backtest assumption | Alert — execution deteriorating |

---

## Maker-Fill Monitoring

### Why It Matters
Backtest assumes 0.04% taker/side. Live FDUSD perpetuals: 0.00% maker. Actual fill mix is the biggest unmeasured variable.
If maker fill is 20% instead of estimated 40-70%:
- Real fee ≈ 3.2bp/side instead of 1.6bp
- Fee-adjusted Sharpe drops from 1.02 → ~0.6 (below deployability threshold)

### Monitoring Protocol
```
Maker fill rate = fills_as_maker / total_fills
Target: ≥40%
Alert:    30-40% — monitor closely
Stop:    <30% for more than 2 trading days
```

### Estimated Fee Impact by Maker Fill Rate
| Maker Fill % | Effective fee/bp | Sharpe estimate |
|-------------|-----------------|----------------|
| 20% | 3.20 | ~0.6 |
| 40% (target) | 2.40 | ~0.8 |
| 60% (optimistic) | 1.60 | ~1.0 |
| 80% | 0.80 | ~1.2 |

---

## Position Sizing Guardrails

### Current (Fixed HSM)
- `HEDGE_SIZE_MULT = 0.25` — reduces all hedge-sized positions to 25% of full size
- Hedge triggers when BTC ATR rank ≥ 65th pctile (HEDGE_ATR_PCT=0.45)
- Max 3 concurrent positions

### Live Monitoring Thresholds
- [ ] No single position > 40% of available margin
- [ ] Total margin used ≤ 60% of account equity at all times
- [ ] Hedge events: log count. If >5 hedge events in 10 trading days, alert (potential chop regime)

---

## Regime Awareness

### Known Stress Regimes
| Regime | Backtest Evidence | Live Risk |
|--------|-------------------|-----------|
| 2026 YTD | Sharpe -5.31, Turtle -22.7% vs BTC +12.7% | High whipsaw risk |
| 2021 chop | Sharpe 0.76, Turtle +35.5% | Repeated stop-outs |
| 2022 bear-chop | Sharpe 0.51, +10.9% | False trend signals |

### Decision Rules
- **2026 YTD regime active?** If BTC 21d ATR rank > 60th pctile AND BTC up >5% while Turtle signals fire → potential divergence. Monitor closely.
- **Chop regime indicator:** >3 consecutive losing trades with hold < 5 bars → likely chop. Pause or reduce position size.
- **Strong trend indicator:** Trade held >20 bars → let Chandelier/Turtle ATR run. Do not exit early.

---

## Post-Halt Review Protocol

Before restarting after any hard stop:
1. [ ] Identify root cause (maker-fill collapse, regime shift, code bug, data issue)
2. [ ] Do NOT restart until root cause is diagnosed
3. [ ] Run `live_bot_exact_equity.rs` with latest data to compare live vs backtest
4. [ ] If equity gap > 30% vs backtest, do not restart without explicit Noah approval
5. [ ] Document incident in `memory/YYYY-MM-DD.md`

---

## M1 Equity Monitor Integration

Run M1 monitor every cron cycle (every 3 hours):
```bash
cargo run --example m1_equity_trajectory_monitor --profile sweep 2>&1 | grep -E "60d return|Status|Tail"
```

Alert triggers from M1:
- Rolling 60d return < 10th historical percentile (-4.5%): 🟡 AMBER alert
- Rolling 60d return < 5th historical percentile (-8.5%): 🔴 RED alert — pause before next trading session
- Equity vs 1y peak < -20%: implicit drawdown warning — monitor closely

---

## Escalation Matrix

| Severity | Condition | Contact | Response Time |
|----------|-----------|---------|--------------|
| 🔴 CRITICAL | Equity drawdown >35% or halving | Arc + Noah immediately | < 1 hour |
| 🟡 WARNING | Maker fill <30%, win rate <38%, API errors >5/day | Noah via Discord | < 24 hours |
| 🟢 INFO | M1 🟡 alert, regime shift indicators | Discord #krypto | Next cron cycle |

**Rule:** When in doubt, pause. Paper results are always upper bounds.
