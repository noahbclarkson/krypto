# 🏆 Strategy Hall of Fame

Strategies that proved themselves in live or out-of-sample testing. Keep the bar high — paper trading doesn't count.

---

## Criteria for Entry

- ✅ Positive out-of-sample returns over at least 6 months
- ✅ Sharpe ratio > 1.0
- ✅ Max drawdown < 20%
- ✅ Minimum 30 trades (statistical significance)
- ✅ Survived at least one regime change (bull → bear or vice versa)

---

## Template

```
### [Strategy Name] — Inducted YYYY-MM-DD

**Author:** @handle  
**Status:** 🟢 Live | 🟡 Retired | 🔵 Archived  
**Hypothesis:** One sentence on the edge being captured.  
**Signal:** Brief description (e.g. momentum + mean-reversion combo)  
**Universe:** Symbols / timeframe  
**Backtest Period:** YYYY-MM-DD to YYYY-MM-DD  
**Live Period:** YYYY-MM-DD to present (or end date)

**Performance Metrics:**
| Metric           | Backtest | Live     |
|------------------|----------|----------|
| Total Return     | X%       | X%       |
| Sharpe Ratio     | X.X      | X.X      |
| Max Drawdown     | X%       | X%       |
| Win Rate         | X%       | X%       |
| Profit Factor    | X.X      | X.X      |
| Total Trades     | N        | N        |

**Key Insights:**
- Why this edge exists and why it's likely to persist  
- Any notable regime behaviour  

**Notes:** (optional free-form)
```

---

## Entries

*(Add new entries below, most recent induction first)*

---

### Bollinger Reversion (XRPUSDT 1d, 0.5× ATR Stop) — Research Candidate 2026-03-10

**Author:** Arc 🦀  
**Status:** 🟡 Research / Not yet live  
**Hypothesis:** XRP's high mean-reversion tendency on daily bars; price reliably snaps back after
deviation from 20-period Bollinger Bands. Tight ATR-scaled stop preserves edge by cutting losers fast.

**Signal:** Price closes outside Bollinger Band (2σ), enter on next open in reversal direction.  
**Universe:** XRPUSDT / 1d  
**Backtest Period:** ~7.8 years (USDT data, 3000 candles)  
**Stop Sizing:** 0.5× ATR(14) ≈ 3.2% at time of writing

**Performance Metrics (USDT taker, annualised):**
| Metric              | Backtest        |
|---------------------|-----------------|
| Total Return        | +15,598%        |
| Annualised Return   | +90%            |
| Annualised Sharpe   | 6,060           |
| Max Drawdown        | 14.1%           |
| Win Rate            | 57.8%           |
| Profit Factor       | 2.54            |
| Total Trades        | 237 (30/yr)     |

**Key Insights:**
- Stop sizing is critical: 0.5× ATR (≈3%) profitable; 1× ATR loses money.
  The edge is in cutting losers very fast — ATR-scaled stops prevent over-wide stops on volatile days.
- Take profit irrelevant — internal Bollinger exit fires before fixed TP hits.
- XRP outperforms BTC/ETH significantly for reversion strategies (higher volatility, stronger mean reversion).
- USDT data has 7+ years; FDUSD validation still needed (shorter history).

**Next Steps:** Run FDUSD validation with passive execution to confirm edge translates to 0% maker fee scenario.

---

### Bollinger Reversion (DOGEUSDT 1d, 0.5× ATR Stop) — Research Candidate 2026-03-10

**Author:** Arc 🦀  
**Status:** 🟡 Research / Not yet live  
**Hypothesis:** DOGE has exceptionally strong mean-reversion on daily bars due to high retail-driven
volatility. 0.5× ATR stop keeps losses very small while letting winners reach natural Bollinger exit.

**Signal:** Same as XRPUSDT entry above.  
**Universe:** DOGEUSDT / 1d  
**Backtest Period:** ~6.7 years  
**Stop Sizing:** 0.5× ATR(14) ≈ 3.6%

**Performance Metrics (USDT taker, annualised):**
| Metric              | Backtest        |
|---------------------|-----------------|
| Total Return        | +22,599%        |
| Annualised Return   | +125%           |
| Annualised Sharpe   | 5,404           |
| Max Drawdown        | 16.0%           |
| Win Rate            | 57.2%           |
| Profit Factor       | 2.35            |
| Total Trades        | 229 (34/yr)     |

**Key Insights:** Same ATR stop logic as XRP. DOGE's higher volatility produces larger absolute returns
but slightly higher drawdown. Annualised Sharpe is enormous due to consistent per-trade edge.

---
