# 2026-04-09 02:38 UTC — DDBudget 4-Sleeve Walk-Forward Validation

**What was tested:**
`examples/ddbudget_4sleeve_walkforward.rs`
Direct A/B test of the DDBudget ensemble allocator, comparing the 3-Sleeve version (A/D, MACD, SmallVol) against the new 4-Sleeve version which includes the recently validated Cross-Sectional Laggard Short.

**Key Findings:**
1. **Pass Rate Drop:** The 4-sleeve version passed 40/54 OOS windows (74%), compared to the 3-sleeve version which passed 46/54 (85%) in previous tests (and 43/54 in the control column of this run due to slightly different start alignment).
2. **Returns Increased:** The 4-sleeve version consistently generated higher aggregate returns across universes (e.g., Base5: +21.3% vs +19.2%; NoDOGE: +25.4% vs +19.0%).
3. **Drawdowns Worsened:** Adding the short sleeve *increased* the average drawdowns across the walk-forward windows. For example, on Base5, the aggregate max DD increased from 9.3% to 13.9%. On Legacy4, it went from 11.8% to 18.0%.

**Honestly:**
The integration failed its primary objective. The Cross-Sectional Laggard Short was designed as "crisis alpha" to *reduce* drawdowns during bear regimes. Instead, injecting a permanent 50% net-short bias into the allocator dragged down the win rate and increased drawdowns (likely due to short squeezes or choppy sideways markets where the laggard mean-reverts instead of continuing down).

**Program Insight:**
You cannot simply bolt a market-neutral/short leg onto a long-only trend budget. The trend sleeves are holding for 21-54 bars (expecting continuation). The laggard short is likely experiencing mean-reversion during those long hold periods.

**Next Steps:**
Do not merge the 4-sleeve DDBudget. Keep the portfolio at 3 sleeves for now. Pivot to testing the macro gates (DXY) as the primary bear-market survival mechanism.
