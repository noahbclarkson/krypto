# Strategy Sweep Results - 2026-03-10

## Expanded Sweep (10 pairs, 1000 combinations)

### Top Performers
1. **DOGE/USDT - Bollinger Reversion (1d)**: +2,173.4% return
   - Config: 5% stop, 0% TP
   - Trades: 223, Win Rate: 49.8%, PF: 1.36, Max DD: 44.4%

2. **DOGE/USDT - Volatility Squeeze (1d)**: +293.0% return
   - Config: 5% stop, 0% TP
   - Trades: 73, Win Rate: 41.1%, PF: 2.14, Max DD: 16.0%

3. **LINK/USDT - Bollinger Reversion (1d)**: +488.4% return
   - Config: 5% stop, 0% TP
   - Trades: 238, Win Rate: 50.8%, PF: 1.22

### Key Findings (from 1000 runs)

**Stop Loss Sensitivity:**
- 5% stop: +77.6% avg return (BEST)
- 8% stop: -43.2% avg return
- 10% stop: -46.9% avg return
- 15% stop: -47.1% avg return
- 20% stop: -45.6% avg return

**Take Profit Sensitivity:**
- TP has negligible impact (all configs around -20% avg return)
- The strategy's internal exit logic fires before fixed TPs are hit

**Timeframe Analysis:**
- 1d dominates: Most profitable configs on daily timeframe
- 4h: Universally unprofitable with current parameters

**Asset Class Ranking:**
1. DOGE (high volatility alts) - BEST
2. LINK
3. SOL
4. AVAX
5. ADA, XRP, DOT
6. BTC, ETH (majors) - WORST for reversion strategies

## Conclusion
- Tight 5% stops are essential - widening destroys the edge
- Take profit doesn't matter - let winners run with internal exit
- Altcoins significantly outperform majors for mean reversion
- Focus on high-volatility assets on daily timeframe
