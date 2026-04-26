# S4: Vol-Adaptive Chandelier (252-bar rank)

**Previous (2026-04-12): 21-bar vol rank → all configs identical → GRAVEYARD**
**This (2026-04-26): 252-bar realized vol rank + DV ranking + pos cap + Sharpe tracking**

| Config | Pass | Avg DD | Trades | Sharpe |
|--------|------|---------|--------|--------|
| M=3.00 | 7/7 (100%) | 33.3% | 587 | 6.89 |
| ADAPTIVE | 7/7 (100%) | 26.5% | 587 | 6.70 |
| M=2.00 | 7/7 (100%) | 25.4% | 587 | 6.68 |
| STATIC M=2.30 | 7/7 (100%) | 28.1% | 587 | 6.59 |
| M=1.75 | 7/7 (100%) | 22.6% | 587 | 6.48 |

**Verdict: TIED — Sharpe difference (+0.12 ADAPTIVE vs STATIC) is within noise (threshold 0.20). No vol-adaptive benefit.**

- ADAPTIVE Sharpe 6.70 vs STATIC M=2.30 Sharpe 6.59 → +0.12, within noise
- All 5 configs pass 7/7 windows (100%)
- All Sharpe values 6.48–6.89 — within 0.41 range
- M=3.00 has highest Sharpe (6.89) but tighter stop (DD 33.3%) vs STATIC (DD 28.1%)
- 252-bar vol rank does NOT provide the slow-vol-regime sensitivity needed
- S4 → GRAVEYARD: vol-adaptive Chandelier is not a useful structural change
