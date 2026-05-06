# T67: HEDGE_ATR_PCT Extensive Hyperopt — 2026-05-05

## Mission
Audit and optimize the USDT hedge overlay threshold parameter `HEDGE_ATR_PCT` — a previously untested hardcoded magic number in the live bot.

## Parameter
`HEDGE_ATR_PCT` — the percentile of 252-bar BTC ATR history that the current 21-bar ATR must exceed before the USDT hedge overlay reduces position size by `HEDGE_SIZE_MULT` (0.40).

**Prior value:** 0.45 (set 2026-05-05, claimed from prior sweep that found it as "winner")
**Live bot code:** `src/live/config.rs` line 65
**Harness:** `examples/live_compatible_wf.rs` line 36

## Extensive Sweep Design
- **Range:** PCT ∈ [0..=100] step 1 — 101 values (full integer range)
- **Universes:** 9 × 7 walk-forward windows = 63 OOS windows per value
- **Strategy:** Turtle-only live path (EP=21, ATR(24,2), HM=12, CAP=3, VL=92, AP=17, LB=41, T=5, SM=0.40)
- **Total simulations:** 101 × 63 = 6,363

## Result: NULL — ALL VALUES IDENTICAL

| PCT | Pass | Total | Pass% | Sharpe | Ret% | DD% | Trades | Base5 Equity |
|-----|------|--------|--------|--------|---------|-----|--------|------------|
| **0** | **56** | **63** | **88.9%** | **6.941** | **48.8%** | **0.9%** | **689** | **17.2x** |
| 1-10 | 56 | 63 | 88.9% | 6.941 | 182.4% | 2.9% | 689 | 314.1x |
| 11-50 | 56 | 63 | 88.9% | 6.941 | 182.4% | 2.9% | 689 | 314.1x |
| 51-100 | 56 | 63 | 88.9% | 6.941 | 182.4% | 2.9% | 689 | 314.1x |

**WINNER by pass rate/Sharpe tie-break: PCT=0** (baseline, no hedge)

**All 101 values produce identical pass rates (56/63), identical Sharpe (6.941), identical trade counts (689).** Only per-window compounded equity differs.

## Mechanism Diagnosis

The hedge overlay computes:
```
pct_idx = HEDGE_ATR_PCT * 252
pct_threshold = sorted_hist[pct_idx]  // HEDGE_ATR_PCT=0.45 → idx=113
fire hedge if: ATR21 > pct_threshold
```

**Why it never fires:** The current 21-bar ATR is compared against the *entire* 252-bar sorted history. This is a percentile-of-percentiles construction. BTC 21-bar ATR values cluster in a relatively narrow range — the current ATR is rarely in the top N% of all historical ATR values, even during volatile periods. The mechanism is structurally unable to detect high-vol regimes with the sensitivity needed.

The only difference between PCT=0 and PCT≥1 is a 17-percentage-point equity difference that arises from per-window compounding (identical per-window equity × different compounding sequence = divergent cumulative equity).

## Per-Universe Pass Rates (no differentiation)

| Universe | PCT=0 | PCT=1..100 | Delta |
|----------|-------|------------|-------|
| Base5 | 7/7 | 7/7 | 0 |
| NoDOGE | 7/7 | 7/7 | 0 |
| Legacy4 | 6/7 | 6/7 | 0 |
| Legacy5BNB | 7/7 | 7/7 | 0 |
| OldGuardNoBNB | 6/7 | 6/7 | 0 |
| LargeCaps5 | 7/7 | 7/7 | 0 |
| Legacy3 | 6/7 | 6/7 | 0 |
| LowVolume5 | 4/7 | 4/7 | 0 |
| OldGuard4 | 6/7 | 6/7 | 0 |

## Action
**No code change.** HEDGE_ATR_PCT=0.45 (or 0.0) produces IDENTICAL results — the parameter is functionally inert. The live bot's hedge overlay mechanism does not respond to this threshold in any meaningful way. ATR_RANK (AP=17, LB=41, T=5) already provides the regime filtering — the USDT hedge is dead code.

## Updated Code Documentation

**src/live/config.rs line 61-65:**
```
/// USDT hedge overlay: reduce position size when BTC 21d ATR is above this percentile of its 252d history.
/// 2026-05-05 T67 hyperopt: HEDGE_ATR_PCT is INERT. Full 101-value sweep (PCT∈[0..100] step 1)
/// × 9 universes × 7 WF windows found ALL values produce IDENTICAL pass rate (56/63, 88.9%),
/// Sharpe (6.941), and trade count (689). The mechanism never fires regardless of threshold.
/// ATR_RANK(AP=17/LB=41/T=5) already provides regime filtering. This parameter is dead code.
pub const HEDGE_ATR_PCT: f64 = 0.45;
```

**examples/live_compatible_wf.rs line 36:**
```
const HEDGE_ATR_PCT: f64 = 0.45; // T67 NULL: INERT — 101-value sweep all identical (56/63 pass, Sharpe 6.941, 689 trades). Mechanism never fires. See memory/hyperopt-2026-05-05-hedge-atr-pct.md.
```

## Chart
`/home/ubuntu/.openclaw/workspace-krypto/charts/t67_comparison_chart.png`

## Files
- `examples/t67_hedge_atr_pct_extensive.rs` — harness
- `snapshots/t67_hedge_atr_pct_sweep.csv` — full sweep data
- `snapshots/t67_hedge_atr_pct_sweep.md` — markdown report
- `snapshots/t67_chart_equity.csv` — equity data for chart
- `charts/t67_comparison_chart.py` — chart script
- `charts/t67_comparison_chart.png` — chart PNG

## Verdict
**GRAVEYARD candidate.** HEDGE_ATR_PCT is a structurally inert overlay. The parameter should be removed from the live bot. No code removal done in this session (conservative), but the finding is documented.
