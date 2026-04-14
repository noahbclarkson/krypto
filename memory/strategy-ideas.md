# Strategy Ideas — Research Pipeline

## Research Program Meta-Analysis — 2026-04-08 05:03 UTC (Kira, Research & Critique Cycle)

### Honest critique of the last 5 research runs
1. **`A/D period=3 9-universe validation`** (dba598e): Correctly caught the A/D period=30 overfitting — period=3 is the real winner. Good. But it was still in the same basin. The DOGE decomposition (also dba598e) was the best structural trust test in the entire program history — it answered whether Base5 was a DOGE story or distributed across symbols. Concluded BTC and SOL are the primary engines.
2. **`CHANDELIER_EXIT` sweep** (713ad44): Tested ATR-based trailing stop vs 21-bar hold. But results were confounded by the HOLD_BARS=49 change that happened in the SAME commit cluster. The comparison was ambiguous — chandelier vs Fixed49 (not Fixed21). This is the second "confounded comparison" in program history.
3. **`DOGE_decomposition`** (dba598e): Exposed that overlapping trade execution (daily re-entry) created a compounding illusion. Under strict non-overlapping walk-forward, BTC (+630.7%) and SOL (+318.4%) are the engines — DOGE is only +102.1%. This is the most important structural finding in weeks.
4. **`LOB daemon deployed`** (1862b57): Started collecting NOBI depth data in background. But the SIGNAL was never tested. The gap between infrastructure and validation is now the longest-unresolved item in the entire program.
5. **`SHORT_SIDE_SLEEVE_DESIGN.md`** (3940e25): Designed on paper but zero commits. Zero code. The entire book is still 100% long-biased.

### Key structural issues
- **Still in the same basin**: 25+ commits on A/D + trend + Small + state overlays. Parameter fishing neighborhood, not genuinely new.
- **Infrastructure vs validation gap**: LOB daemon collecting, but never tested. FDUSD carry invalidated cleanly (good), but the carry stub was never built. The gap between "written" and "tested" is too wide.
- **Short-side sleeve**: zero commits, zero code. Biggest structural gap in program history. Not even a stub.
- **Mean reversion still dead**: RSI (graveyard), BollingerReversion (graveyard), OFI proxy (graveyard), VPIN crash-state (graveyard). BTC-ETH cointegration (proposed, never built).
- **Allocator ahead of alpha**: DDHard with complex tier management on a one-trend-engine book.

### Most promising genuinely new ideas (this cycle)
1. **Market-cap-normalized order flow (arxiv 2512.18648)** — "Refactoring order imbalance alphas to use market cap normalization could yield 30% or greater Sharpe ratio improvements." This is the upgrade to the dead OFI proxy. Instead of raw volume imbalance, normalize by market cap. Binance depth endpoint is free. The SG-smoothing step is the same but normalization is the key improvement.
2. **LOB NOBI signal from already-collected data** — daemon has been running since 1862b57. We have depth data sitting in a file. One harness run to test the signal. This is the lowest-lift highest-value test in the entire pipeline.
3. **BTC-ETH cointegration pair trading** — first credible mean-reversion object. Completely orthogonal to everything in the current book. January 2026 Frontiers paper. Rolling Johansen method.
4. **ETF flow as macro regime filter** — $1.2B ETF inflows in first two trading days of 2026 signal institutional demand. 5-day rolling outflow is a hard risk-off filter. TheBlock, TheBlock.co have free daily flow data. Zero institutional data in the current program.
5. **Leverage-fragility state (Oct 2025 mechanics)** — $19B OI erased in 36 hours. Funding > 0.05%/hour = fragile. Post-cascade reset = rebound window. We have the funding data cached.

### Biggest blind spot
Still no short-side family, no microstructure validation, no mean-reversion. The program is structurally one-directional.

### If deploying real money tomorrow
Only a small, throttled, cash-aware version of the frozen three-sleeve book. Not the overlay stack. Not the pressure promotion. Not the allocator complexity. Treat everything as research-grade.

### What a skeptical fund quant would say
"You have a sophisticated risk allocator running on top of one trend engine and a partial A/D diversifier. The three-sleeve book is more correlated than your Sharpe tables suggest — the DOGE decomposition proves BTC and SOL do most of the work. Your biggest miss is the lack of short-side and microstructure. The LOB daemon has been running for days and you still haven't tested the signal. Open a new information lane or stop optimizing the same one."

---

_Last updated: 2026-04-07 08:34 UTC (Kira, research & critique cycle — macro prediction markets, cross-sectional momentum structure, and ETF institutional flow)

---
### LOB Depth Imbalance with Savitzky-Golay Smoothing (arxiv 2602.00776)
- **Source:** arxiv 2602.00776 "Explainable Patterns in Cryptocurrency Microstructure" (Jan 31, 2026) + arxiv 2506.05764 (June 2025 Best Paper runner-up)
- **Edge hypothesis:** Wang (Jan 2026) documents STABLE cross-asset LOB patterns across BTC, ETH, SOL spanning different market caps. The key finding: simpler models with good preprocessing outperform deep architectures on raw LOB data — preprocessing is the bottleneck. Concrete features: top-5-level depth imbalance `(bidQty - askQty)/(bidQty + askQty)`, spread dynamics, volume-weighted order arrival rates. These aggregate cleanly to daily without microstructure noise.
- **Data requirements:** Binance Depth API (`/api/v3/depth`, top 20 levels, no auth). Aggregate snapshots at 5min or 15min intervals to daily. BTC/ETH/SOL first.
- **Complexity:** medium — data collection + SG smoothing + z-score signal. But Binance depth endpoint is free and unauthenticated.
- **Novelty vs current work:** very high — properly-constructed microstructure proxy using LOB resting liquidity; fundamentally different from dead OHLCV-based OFI attempt.
- **Minimum viable test:** aggregate Binance depth snapshots to 1h; compute `(bidQty_1-askQty_1)/(bidQty_1+askQty_1)` at top level; smooth with 5-day SG filter; test if daily NOBI > threshold predicts next-24h directional continuation vs frozen three-sleeve baseline.
- **Priority:** HIGH — most rigorous microstructure paper in the 2025-2026 literature; directly addresses the VPIN/OFI failure with proper instrument choice; Binance public endpoint available NOW
- **Status:** proposed — NEW 2026-04-07 PM

### Options IV Skew as Institutional Fear Gauge (Deribit/DVOL)
- **Source:** CME Group Feb 2026 research; CoinLaw.io (Feb 2026); Deribit BTC DVOL index
- **Edge hypothesis:** CME Group confirms: "A significant surge in Bitcoin options trading preceded the onset of the sell-off on January 29, 2026." DVOL (Deribit's 30-day annualized implied vol) at 45-50 and rising = institutional positioning for volatility. But the specific signal is the 25-delta put/call IV spread: when OTM puts IV >> OTM calls IV (negative skew), institutions are buying crash insurance — this PRECEDES drawdowns. When skew normalizes (puts cheap), it marks post-stress recovery. Use skew as a binary regime gate: high-skew state → reduce trend exposure or add inverse exposure.
- **Data requirements:** Daily DVOL index + 25-delta put/call IV spread for BTC. Deribit publishes this publicly. Free availability confirmed by CME research.
- **Complexity:** low-medium — data fetch + skew computation + regime gate. DVOL is a single index number.
- **Novelty vs current work:** very high — genuinely forward-looking institutional derivatives signal; completely different from all backward-looking price/volume signals; introduces the OPTIONS market as a new information layer.
- **Minimum viable test:** collect historical DVOL + skew; label days as `high-skew / normal / low-skew`; compare next-21-bar three-sleeve returns conditioned on high-skew vs normal across same 9 universes. If high-skew state precedes worse trend returns, use as selective de-risk trigger.
- **Priority:** HIGH — institutional-grade signal with published CME validation; forward-looking nature is unique in our entire signal set
- **Status:** proposed — NEW 2026-04-07 PM

### DXY-Realized-Vol Regime Gate (BTC Now a Liquidity-Sensitive Risk Asset)
- **Source:** JPMorgan 2026 analysis via ainvest.com; CME Group May 2025 research; Stoic.ai (Jan 2026)
- **Edge hypothesis:** JPMorgan confirms BTC's DXY correlation has shifted to POSITIVE in 2026 — BTC now behaves as a liquidity-sensitive risk asset, NOT a dollar hedge. This is a structural regime change from pre-ETF era. Combined with elevated BTC-SPX correlation (0.5-0.88), the regime that matters is now "DXY strengthening = crypto headwind" rather than the old "DXY weakness = crypto tailwind." Use rolling DXY change + realized vol as a compound regime gate: when DXY is rising AND BTC vol is elevated → bearish macro regime for crypto.
- **Data requirements:** DXY daily data (FRED or public yfinance) + BTC daily realized vol. Both accessible.
- **Complexity:** very low — single composite gate: `(DXY_Δ > 0) AND (BTC_vol_zscore > 1.0)` = bearish macro regime. 
- **Novelty vs current work:** high — structural shift in BTC's macro sensitivity not captured by any existing signal; our macro overlay used SPX which is correlated but not the primary channel.
- **Minimum viable test:** compute daily DXY change; condition the frozen three-sleeve on `bearish_macro = (DXY_Δ > 0 AND BTC_vol_z > 1.0)`; benchmark against unconditioned baseline on same 9 universes; if bearish macro precedes reliably worse 21-bar returns, use as position-size throttle.
- **Priority:** HIGH — addresses the most significant structural change in BTC market behavior (ETF-era correlation shift); uses public free data; immediately actionable
- **Status:** proposed — NEW 2026-04-07 PM

### NEW — 2026-04-07 08:34 UTC (Morning Research & Critique — 3 New Ideas)

**Kalshi Macro Contract Repricing (Prediction Market Uncertainty)**
- **Source:** arXiv 2604.01431 (April 2026): "Do Prediction Markets Forecast Cryptocurrency Volatility? Evidence from Kalshi Macro Contracts"
- **Edge hypothesis:** Daily probability changes in regulated prediction markets (Kalshi KXFED, KXCPI, KXRECSSNBER contracts) forecast crypto volatility better than scheduled macro releases. Bitcoin responds to monetary policy repricing (Fed-dovish signals during cutting cycles), while altcoins (ETH, SOL, LINK) respond more strongly to inflation regime uncertainty (CPI repricing). This is a completely new class of continuous, forward-looking macro signal.
- **Data requirements:** Kalshi daily contract prices for Fed rate decisions and CPI. Daily probability first-differences.
- **Complexity:** medium — requires external data fetch from Kalshi or proxy prediction markets.
- **Novelty vs current work:** completely new. We have no continuous macro expectation signals in the book right now.
- **Minimum viable test:** fetch Kalshi KXFED / KXCPI historical daily probabilities; compute first-differences; test if large daily repricing predicts next-5d BTC/ETH realized volatility and whether the volatility is directional.
- **Priority:** MEDIUM — data dependency is the main blocker, but theoretically very strong.
- **Status:** proposed

**Cross-Sectional Factor Momentum (Time-Series of Cross-Sectional Factors)**
- **Source:** SSRN/TandF (2023-2025): "Cryptocurrency factor momentum: past winners consistently outperform losers."
- **Edge hypothesis:** Instead of pure time-series momentum on the assets themselves, trade momentum on the FACTORS. If the "small cap" factor or "value" factor was the best performing factor over the last 14 days, allocate capital to that factor for the next 7 days. This uses our existing cross-sectional sleeves but adds an active allocation layer based on factor momentum, replacing static equal weight.
- **Data requirements:** None. Uses existing sleeve return streams.
- **Complexity:** low.
- **Novelty vs current work:** We currently use a static or naive overlap allocator. This is dynamic factor rotation.
- **Minimum viable test:** compute rolling 14d return of A/D sleeve, MACD sleeve, and FactorSmall sleeve. Each week, allocate 100% to the sleeve with the highest 14d momentum. Compare to equal weight.
- **Priority:** HIGH — uses existing code, directly improves the allocator.
- **Status:** proposed

**Bitcoin ETF Flow Confirmation (Institutional Margin)**
- **Source:** April 2026 ETF flow analysis (CoinGlass, Amberdata). ETF flows don't predict turning points, they *confirm* trends. Sustained outflows (5+ days) strongly signal institutional regime shifts. Q1 2026 saw $46.7B YTD inflows but recent market fragility.
- **Edge hypothesis:** Institutional ETF flow (IBIT, FBTC, ARKB) is the marginal driver of BTC price in 2025-2026. A 5-day rolling net outflow is a reliable macro-regime warning that overrides all technical trend signals. Rather than using it as a long signal, use sustained ETF outflow as a hard risk-off filter for the entire portfolio.
- **Data requirements:** Daily Spot BTC ETF net flows (publicly available via CoinGlass or Farside APIs, published T+1 morning).
- **Complexity:** low/medium — requires new daily flow data, but logic is simple.
- **Novelty vs current work:** Institutional flow data is entirely absent from our current state objects.
- **Minimum viable test:** Collect daily net ETF flow from Jan 2024 to Apr 2026. Compute 5-day rolling sum. Test if 5-day net outflow < 0 provides a better regime filter than our price-based SMA200 or Volatility state.
- **Priority:** HIGH — addresses the biggest blind spot in modern crypto structure.
- **Status:** proposed

---
### NEW — 2026-04-07 04:11 UTC (Deep Research & Critique — 4 New Ideas)

**Adaptive Chandelier-Style Trailing Stop with Time Floor**
- **Source:** internal meta-critique — 30+ commits, 21-bar fixed hold NEVER tested. Chandelier Exit (Chandes 2012) uses ATR trailing stop: `trail = highest_high_since_entry - (ATR × multiplier)`. Combined with a minimum hold floor (e.g., floor after 10 bars) prevents trend-following strategies from exiting winners too early while cutting losers faster than time-based holds.
- **Edge hypothesis:** A trailing stop that exits when price crosses below `(N-bar high - A×ATR)` captures more of trending moves without letting losers run. The floor prevents the trailing stop from firing immediately after entry during normal pullbacks. The minimum hold floor (e.g., 10 bars minimum before trailing activates) is the crucial addition — pure Chandelier fires immediately on entry, which is wrong for our use.
- **Concrete design:** entry → ATR-based stop activates after `min_bars` (10-14); trail updates each bar when `high > trail`; exit when `close < trail`. Sweep A ∈ [2.5, 3.0, 3.5, 4.0], min_bars ∈ [8, 10, 14].
- **Data requirements:** none new — uses same OHLCV. Changes exit logic only.
- **Complexity:** low — one new function in backtest engine, ~50 lines.
- **Minimum viable test:** take frozen three-sleeve book, replace 21-bar hold with Chandelier(A=3.0, min_bars=10); benchmark vs fixed-hold baseline across 9 universes. Single commit.
- **Priority:** HIGH — most overdue structural test in entire program history
- **Status:** proposed — NEW 2026-04-07

**ETH FDUSD/USDT Basis Carry (Confirmed 0% Maker Advantage)**
- **Source:** Binance listing data confirms ETHFDUSD has 0% maker fee (maker -0.01%, taker 0.02%). FDUSD perpetuals have structural maker advantage vs USDT perpetuals (0.02% taker vs 0.05%). Combined with basis carry = direction-neutral. ETH has more FDUSD history than SOL.
- **Edge hypothesis:** ETH FDUSD perpetual (0% maker) vs ETH USDT perpetual (0.02% taker): long ETH spot, short ETH FDUSD perpetual earns: (basis premium) - (funding cost differential). With FDUSD funding historically near 0%, the basis carry should be positive. This is different from perp-perp (which was funding-as-momentum) and different from perp-spot (which was directional). Basis carry should be market-neutral direction, earning the contango premium.
- **Data requirements:** ETHFDUSD_4h (cached), ETHUSDT_4h (cached), ETH funding history. All available.
- **Complexity:** medium — needs basis calculation and carry accounting.
- **Novelty vs current work:** very high — market-neutral, non-directional, completely different from all current strategies.
- **Minimum viable test:** compute daily ETH FDUSD vs USDT basis from 4h data; track carry = basis - funding_cost; test whether carry > 0 historically; if yes, long ETH spot + short ETH FDUSD perp backtest with next-open execution.
- **Priority:** HIGH — data already cached, directly actionable
- **Status:** proposed — NEW 2026-04-07

**DOGE Systematic Decomposition: Why Does DOGE Drag EVERYTHING?**
- **Source:** internal — DOGE consistently fails: intraday MR (DOGE GRAVEYARD, 40% WR), OFI momentum (DOGE corrupted rankings), perp-perp carry (DOGE short leg worst), and yet DOGE is in the Base5 universe that produces the best headline returns. The DOGE problem is universal across factor families but we have never diagnosed it specifically.
- **Edge hypothesis:** DOGE's high meme-driven retail vol creates price dynamics that violate the assumptions of ALL factor families: trend (DOGE whipsaws), mean reversion (DOGE overshoots), flow (DOGE flow is noise), carry (DOGE funding is extreme and mean-reverts on its own timeline). This means every strategy including DOGE would be STRONGER on NoDOGE universes. But DOGE also provides extreme volatility that amplifies returns when it works. The question is whether the drag exceeds the amplification.
- **Data requirements:** none new — DOGE data already in cache.
- **Complexity:** very low — per-symbol PnL decomposition across all strategies.
- **Minimum viable test:** compute per-symbol contribution to total PnL for A/D, MACD+Regime, FactorSmall on Base5 vs NoDOGE; if DOGE drag > DOGE amplification across all three families, the honest answer is NoDOGE as default universe.
- **Priority:** MEDIUM — structural question about our universe definition; answers whether Base5 is actually the right benchmark
- **Status:** proposed — NEW 2026-04-07

**Walk-Forward A/D Period Validation Across 9 Universes**
- **Source:** internal — A/D hyperopt (fde1b49) found period=30 beats period=20 by +26% Sharpe on Base5. But it was ONLY tested on Base5. The improvement could be Base5-specific and not hold on hostile/legacy universes. This is the same error pattern we want to avoid.
- **Edge hypothesis:** The A/D period 20→30 improvement might be a lucky Base5 artifact. Period 20 might actually be better on hostile/legacy sets. We need to test BOTH periods across ALL 9 universes before committing to period=30.
- **Data requirements:** none new.
- **Complexity:** very low — run the existing AD walk-forward on all 9 universes with period=20 vs period=30.
- **Minimum viable test:** extend `examples/ad_period_hyperopt.rs` or `examples/ad_accumulation_walk_forward.rs` to run both periods across all 9 universes; compare pass rate, avg OOS Sharpe, avg OOS return for each.
- **Priority:** HIGH — this is a committed change (period already changed to 30 in code) that needs 9-universe validation before it becomes the frozen baseline
- **Status:** proposed — NEW 2026-04-07

---

### NEW — 2026-04-06 Evening Research Sweep (Updated 21:22 UTC)

**Adaptive Trailing Exit (Chandelier ATR) — Replaces Fixed 21-Bar Hold**
- **Source:** arXiv 2512.20366 (Dec 2025): Chandelier Exit with A=2.5, B=60 achieves 9.3% CAGR on S&P 500 vs 7.1% buy-and-hold. Crypto has higher volatility so ATR multiples (2.5-4.0) are natural. Our 21-bar fixed hold is the worst part of the execution model — it forces exits on winning trends too early and holds losers past reversals.
- **Edge hypothesis:** Replace fixed 21-bar hold with ATR trailing stop: trail high minus (ATR × multiplier) on bars since entry. Exit when close crosses below trail. This captures more of the winning trend (no time cutoff) while cutting losers quickly. Multipliers: 2.5-4.0 (wide enough for crypto noise). Minimum lookback: 14 bars for stable ATR.
- **Data requirements:** none new — uses same OHLCV data. Just changes exit logic.
- **Complexity:** low
- **Novelty vs current work:** high — we've spent 30+ commits fixing entries while using the same dumb fixed-bar exit. This is the lowest-hanging structural improvement that hasn't been tested once.
- **Minimum viable test:** take frozen three-sleeve book, replace 21-bar hold with Chandelier Exit (A=3.0, lookback=14). Compare Sharpe, MaxDD, avg trade return, trade count on same 9 universes. Sweep A=[2.5, 3.0, 3.5, 4.0] in single run.
- **Priority:** HIGH — most likely single-commit improvement to the existing book
- **Status:** proposed — NEW 2026-04-06

**Perp-Spot Basis Carry (FDUSD Perp vs USDT Spot)**
- **Source:** BIS Working Papers No 1087 ("Crypto Carry"); Raison (Feb 2026): annualised futures basis averaged 20-25% vs 5% SOFR. We have `btcfdusd_4h.parquet` (FDUSD perpetual) and `btcusdt_4h.parquet` (USDT spot) — directly usable for basis calculation without new data collection.
- **Edge hypothesis:** BTC and ETH FDUSD perpetuals consistently trade at a contango premium to USDT spot (funding rate = basis proxy). Long USDT spot, short FDUSD perpetual → earns basis minus financing cost. Structurally different from our dead perp-perp carry (which was momentum in disguise) and different from directional strategies. Non-directional, market-neutral carry.
- **Data requirements:** Already cached: `btcfdusd_4h.parquet` and `btcusdt_4h.parquet` — same-symbol different-leg. Basis = `FDUSD_close - USDT_close` annualized by 365/hours_to_expiry. Can also use funding_rate as basis proxy.
- **Complexity:** medium — needs margin/financing modeling; carry = basis - funding_cost. FDUSD has 0% maker, 0.02% taker (vs 0.1% for USDT perps).
- **Novelty vs current work:** very high — genuinely different from every directional strategy; market-neutral carry orthogonal to the entire existing book
- **Minimum viable test:** compute daily basis from FDUSD-USDT spread; track carry = basis - funding_cost; test whether carry > 0 on average; if yes, build long-spot-short-perp backtest. Even a simple "carry when basis > X bps" filter on the existing book would add a new dimension.
- **Priority:** HIGH — directly actionable with cached data; market-neutral carry is a genuinely different edge type
- **Status:** proposed — NEW 2026-04-06 (verified: data exists in cache)

**Cross-Timeframe Ensemble: ETH 4h Mean Reversion + BTC 1d Trend**
- **Source:** internal — ETH 4h MR (+580% OOS, 545 trades, 74% WR); BTC 1d trend (MACD+Regime best all-around). Timeframe orthogonality: intraday mean reversion wins crash windows; daily trend wins bull windows. Combining them is diversification by TIME HORIZON, not just symbol.
- **Edge hypothesis:** ETH 4h MR and BTC 1d trend operate at genuinely different timescales. When both signal the same direction → stronger conviction. When they disagree → the daily trend is the primary signal (longer horizon dominates). This is fundamentally different from simple symbol diversification — it's diversifying the ALPHA HORIZON itself.
- **Data requirements:** ETH 4h bars (already cached), BTC 1d bars (already cached). ETH MR signal at 4h, BTC trend signal at 1d.
- **Complexity:** low — no new data; just ensemble logic: both agree → full size; BTC agrees, ETH disagrees → reduce; ETH only → no trade.
- **Novelty vs current work:** high — we have never combined timeframe-differentiated strategies; all three sleeves (A/D, MACD, Small) are same-timeframe
- **Minimum viable test:** run ETH 4h MR signals + BTC 1d MACD+Regime on same window; measure correlation of signals; compare ensemble vs BTC-daily-only on same 9 universes
- **Priority:** HIGH — immediately actionable; directly combines our two best recent findings
- **Status:** proposed — NEW 2026-04-06

**Realized Vol Signal as Regime State**
- **Source:** internal — realized vol-of-vol is the signal that broke BOCPD and EWMA-CUSUM; vol-of-vol was informative (NIG gave inverted signal). Simple vol% rank vs 252d history was informative in vol-regime sizing. A simpler standalone realized-vol Z-score signal might work better than complex multi-factor state maps.
- **Edge hypothesis:** BTC 21-bar realized vol %-rank vs 252-bar history is the single best and simplest state signal available. Use it directly as a regime filter rather than building complex multi-input state machines: when BTC realized vol > 80th percentile → reduce trend exposure; when < 20th percentile → normal exposure. Simple, interpretable, no overfitting possible.
- **Data requirements:** BTC daily close series only — same data we already use for regime filtering
- **Complexity:** very low — single metric, no multi-factor combination
- **Novelty vs current work:** different from complex EWMA-CUSUM/BOCPD; this is "vol regime as simple as possible but no simpler"
- **Minimum viable test:** compute BTC 21-bar vol %-rank; filter daily trend entries when vol > 80th pct; compare vs unfiltered baseline on same 9 universes. Simple, falsifiable, honest.
- **Priority:** MEDIUM — simple enough to be robust; should have been tested before BOCPD complexity
- **Status:** proposed — NEW 2026-04-06

**ETF Flow Momentum — Institutional Inflows as Alpha**
- **Source:** CoinDesk (Nov 2025): spot Bitcoin ETFs surpassed $100B net inflows with record $3.4B daily; Bitwise (Jan 2026): Bitcoin remains mispriced as macro conditions improve; VanEck (Jan 2026): uses specific onchain metrics to manage entry risk; The Block (Jan 2026): ETF inflows stabilize prices amid thin liquidity.
- **Edge hypothesis:** Spot BTC/ETH ETF net flows are observable daily (public 19b-4 filings, published next morning). Sustained positive net flows = institutional accumulation = bullish on 1-4 week horizon. Sustained outflows = institutional distribution = bearish. The flow data is public, free, and updates daily. This is a genuinely NEW non-price data source.
- **Data requirements:** ETF flow data — either scraped from public 19b-4 filings, or from a free API (some aggregators publish this daily). BTC ETFs: IBIT, FBTC, ARKB, BITB, SOL ETFs: unknown (none approved yet?). Focus on BTC first.
- **Complexity:** medium — data collection/scraping is the main cost
- **Novelty vs current work:** extremely high — zero institutional flow data in any existing strategy; completely new data lane
- **Minimum viable test:** collect ETF flow timeseries (manual scrape or API-free source); build rolling 5-day/10-day net flow z-score; test whether positive flow z-score predicts positive BTC next-5d and next-21d returns. If correlation holds, build overlay sleeve.
- **Priority:** HIGH — genuinely new data, institutional-level signal
- **Status:** proposed — NEW 2026-04-06 (data source needs identification)

---
### NEW — 2026-04-05 Evening Research Sweep

**Intraday Mean Reversion Walk-Forward (ETH+XRP 4h bars)**
- **Source:** internal — intraday_mean_reversion_benchmark (f46881a): ETH 123 trades / 4 windows / +8.6% avg OOS; XRP 64 trades / 3 windows / +5.2% avg OOS. These are the most credible new signals found in the entire program history.
- **Edge hypothesis:** 1h bars are thin (6-7 trades per window for BTC/SOL). 4h bars give more bars per config search without averaging away the short-horizon mean reversion. ETH+XRP ensemble (both credibly positive at 1h) is more robust than either alone.
- **Data requirements:** 4h klines for BTC, ETH, SOL, XRP, DOGE via existing Binance loader. ~2 years of data should be available.
- **Complexity:** low
- **Novelty vs current work:** very high — this is the first confirmed intraday edge in the entire program; everything else is daily resolution.
- **Minimum viable test:** rolling z-score at 4h on ETH+XRP; entry z > 2.0, exit z < 0.3; compare multi-window OOS Sharpe/DD; ETH+XRP ensemble vs BTC-only baseline.
- **Priority:** HIGH — most credible live result in program history
- **Status:** proposed — REQUIRES walk-forward validation, not just a first pass

**EWMA-CUSUM Overlay Harness (Higher Thresholds)**
- **Source:** internal — ewma_cusum_break_detection (aea077d): Mid config gives 94% Break+PostBreak at th=4.0. Need th=10-15 to get break fraction down to 5-15% range. Evidence stream is validated; detection harness is just miscalibrated.
- **Edge hypothesis:** The EWMA-CUSUM evidence stream IS informative (COVID=-1.31, LUNA=-0.23, BTC Nov-2021=+2.55). The problem is threshold calibration, not signal quality. Higher thresholds should produce a more useful break/post-break/stable tripartite state.
- **Data requirements:** none new — same BTC return stream.
- **Complexity:** low
- **Novelty vs current work:** direct fix of the only correctly-validated state detection signal in the program.
- **Minimum viable test:** sweep th in [6, 8, 10, 12, 15]; measure break fraction and stressed-regime next-21-bar return; pick best threshold; build overlay harness.
- **Priority:** HIGH — only validated informative evidence stream; deserves proper calibration
- **Status:** proposed — REQUIRES threshold recalibration before any overlay work

**Leverage-Fragility State with Real Binance Funding Data**
- **Source:** Oct 2025 $19B liquidation cascade (SSRN); Amberdata Q4 2025 leverage report — funding >15% APR = warning signal; >20% APR = R4/R5 fragile; liquidation intensity >5% of OI = cascade threshold (Oct 2025 reached 4.82%); Bitmex Q3 2025: institutional capital (Ethena) actively enforces funding rate ceiling — this is WHY perp-perp carry failed: shorting high-funding assets means fighting institutional arb capital.
- **Edge hypothesis:** Use Binance 8h funding from `data/funding_cache/*.parquet`; compute daily funding percentile for BTC, ETH, SOL. Amberdata thresholds (15% and 20% APR) are directly usable as calibrated state boundaries. Post-cascade calm states (funding reset to <5%) = rebound window.
- **Data requirements:** `data/funding_cache/*.parquet` — already cached; OI daily from Binance public API (~31 rows, shallow but usable for recent windows).
- **Complexity:** low-medium
- **Novelty vs current work:** very high — real derivatives-state lane; explains why perp-perp carry failed structurally; directly actionable with existing data.
- **Minimum viable test:** build daily leverage-fragility index from funding percentile + realized_vol z-score; label R4/R5 fragile when funding > 20% APR; compare frozen three-sleeve next-21-bar returns post-fragile vs calm.
- **Priority:** HIGH — explains a structural failure and is directly testable with cached data
- **Status:** proposed — REQUIRES proxy bug fix (was using btc_volume as funding proxy)

**SAC/DDPG Position Sizing on Frozen Sleeves (Constrained RL)**
- **Source:** arXiv 2511.20678 (November 2025): "Cryptocurrency Portfolio Management with Reinforcement Learning: Soft Actor-Critic and Deep Deterministic Policy Gradient Algorithms" — SAC with entropy regularization outperforms DDPG in noisy crypto markets; SSRN systematic review (April 2025): RL shows steady improvement in crypto applications.
- **Edge hypothesis:** RL is dangerous for signal discovery but potentially useful as a constrained sizing wrapper. Action space limited to discrete exposure levels [0.25, 0.5, 0.75, 1.0] for each sleeve + cash. Heavy penalties: turnover cost, drawdown penalty, Sharpe downside. MUST NOT let RL pick assets or generate signals.
- **Data requirements:** existing sleeve return streams, observable state inputs, strict chronology split.
- **Complexity:** high
- **Novelty vs current work:** different from all rule-based allocators; strictly sizing-only so overfitting risk is contained.
- **Minimum viable test:** implement SAC choosing discrete A/D, MACD, Small, cash exposure; heavy regularization; compare against DDHard and equal-weight out-of-sample on same 9 universes. CRITICAL: RL cannot pick assets or generate signals.
- **Priority:** MEDIUM
- **Status:** proposed — validated by Nov 2025 literature

**Stablecoin Exchange Reserve State (Binance Public API)**
- **Source:** Coinpaper (April 2026): Binance ERC20 stablecoin reserve increasing = stronger available liquidity signal; CryptoQuant exchange whale ratio; Binance public coin 总仓储 endpoint accessible without auth.
- **Edge hypothesis:** USDT/USDC exchange balance changes relative to total stablecoin supply mark whale distribution vs accumulation. Rising stablecoins on exchange = whales preparing buying firepower (bullish). Falling exchange balances = accumulation off-exchange (bullish longer-term, near-term selling risk). Concrete: `exchange_usdt_balance / 30d_avg_exchange_balance`.
- **Data requirements:** Binance public API — need to find correct endpoint for exchange reserve/coin warehouse balance history. `GET /sapi/v1/asset/currencycanDeposit/address` or aggregated reserve history. Daily USDT + USDC exchange balance. No auth required.
- **Complexity:** low-medium
- **Novelty vs current work:** very high — completely different non-price data lane; whale flow orthogonal to all price/volume/derivatives signals tested.
- **Minimum viable test:** compute rolling 7d/30d exchange balance ratio for USDT on Binance; label accumulating vs distributing; test next-7d and next-21d BTC returns conditioned on state.
- **Priority:** HIGH — genuinely new data lane, no auth required
- **Status:** proposed — API endpoint needs to be identified

**Short-Side Sleeve Prototype (Biggest Structural Gap)**
- **Source:** internal program critique — entire book is 100% long-biased trend direction; no way to express bearish or defensive view; KuCoin/Bitcoinist bear-market literature (2026) confirms directional shorting remains active.
- **Edge hypothesis:** A short-side sleeve gives the book genuine crisis-alpha. The honest version is NOT standalone directional shorting — it is a conditional short used only when EWMA-CUSUM evidence drops below -1.0 (stressed regime, where stressed regime has consistently negative next-21-bar BTC returns per aea077d). Use as selective de-risk / inverse exposure when BOCPD evidence is deeply negative.
- **Data requirements:** BTC 1d or 8h returns; existing EWMA-CUSUM evidence stream; Binance funding data.
- **Complexity:** medium
- **Novelty vs current work:** very high — single biggest structural gap in the current book.
- **Minimum viable test:** add simple short sleeve (inverse-BTC when EWMA-CUSUM evidence < -1.0) to frozen three-sleeve book; compare against long-only baseline on same 9 universes; explicit attention to bear-market windows.
- **Priority:** HIGH — biggest structural blind spot
- **Status:** proposed — needs EWMA-CUSUM calibration first

---

### NEW — 2026-04-05 Research Cycle Additions

**Stablecoin Exchange Whale Ratio (Binance Public API — No Auth Required)**
- **Source:** CryptoQuant Exchange Whale Ratio; Bitget research (Jan 2026); Nansen whale-flow literature — exchange whale ratio = top-10 inflows / total inflows; readings above 85% preceded 30%+ drawdowns in 2024-2025.
- **Edge hypothesis:** USDT/USDC exchange balance changes relative to total volume mark near-term sell pressure. Rising stablecoin exchange balances = whales preparing buying firepower (bullish). Falling exchange balances = whales accumulating off-exchange (bullish longer-term, near-term selling risk). This is tractable without any API key — use Binance's public coin 总仓储 (coin warehouse) balance history endpoint.
- **Data requirements:** Binance public API — `sapi/wallet/capitalConfig/getall` for USDT/USDC exchange flow; daily resolution; BTC/ETH/SOL/USDT pairs first.
- **Complexity:** low-medium
- **Novelty vs current work:** very high — stablecoin flow is genuinely different from price/volume/derivatives; completely new lane; whale ratio above 85% is a concrete, tested threshold from practitioner literature.
- **Minimum viable test:** build daily exchange-whale-ratio state (`whale_distribution`, `whale_accumulation`, `neutral`); compare next-7d and next-21d sleeve returns conditioned on state; test whether whale-distribution state precedes worse sleeve returns (de-risk trigger) and whale-accumulation precedes better (opportunistic entry augment).
- **Priority:** HIGH
- **Status:** proposed — NEW 2026-04-05

**EWMA-CUSUM Break Detection (Overlay Harness)**
- **Source:** internal — BOCPD evidence stream fix (aea077d); evidence is now correctly informative (COVID=-1.31, LUNA=-0.23, BTC Nov-2021=+2.55), but NIG conjugate model never fires Break reset (0% Break, run-length always pegged at 13).
- **Edge hypothesis:** The EWMA-CUSUM approach is better suited than BOCPD's NIG model for detecting vol-of-vol breaks. The evidence stream IS informative — use it as the error signal in a CUSUM device to generate discrete break flags. More responsive than BOCPD (hazard 0.08 ≈ 12-bar expected run-length), and more appropriate for the evidence structure.
- **Data requirements:** none new — same BTC return stream used for BOCPD evidence.
- **Complexity:** low
- **Novelty vs current work:** direct fix of BOCPD approach; evidence is already validated as informative; this is the missing piece for overlay harness.
- **Minimum viable test:** build EWMA-CUSUM break detector from evidence stream; generate `Normal / Stale / Break` flags; use as BOCPD-regime overlay harness; benchmark stressed-regime next-21-bar returns vs flat baseline on same 9 universes.
- **Priority:** HIGH
- **Status:** proposed — NEW 2026-04-05 (replaces BOCPD NIG as primary break-detection approach)

**Perp-Perp Funding Rate Cross-Sectional Carry (No Spot Leg)**
- **Source:** Bitmex Q3 2025 Derivatives Report; MDPI January 2026 `Two-Tiered Structure of Cryptocurrency Funding Rate Markets` (35.7M observations, 26 exchanges); perp funding gravitates toward 0.01%/8h anchor; institutional arbitrage enforces it; perp-perp spread is the cleaner carry.
- **Edge hypothesis:** When SOL funding > BTC funding by >X bps/8h, short SOL/long BTC perp. No spot leg, no large capital, no financing. The convergence mechanic is the 0.01% anchor, not price-level convergence. More concretely: cross-sectional funding rank drives a carry sleeve independent of price direction. This is different from dead perp-spot carry and different from funding-as-directional-signal.
- **Data requirements:** 8h funding rates for BTC, ETH, SOL, XRP, ADA — in `data/funding_cache/*.parquet`. Binance rates are CEX-anchored (61% more informative than DEX per MDPI Jan-2026 paper).
- **Complexity:** low-medium
- **Novelty vs current work:** very high — perp-perp carry is a derivatives-term-structure lane; completely different from price/volume or funding-as-directional-signal.
- **Minimum viable test:** compute rolling 8h funding differential (SOL-BTC, ETH-BTC, etc.); rank symbols by funding; go long bottom-funding / short top-funding per period; 21-bar hold; compare against flat/baseline on ETH-containing universes.
- **Priority:** HIGH
- **Status:** proposed — NEW 2026-04-05

**Short-Side / Bear-Market Alpha (The Biggest Blind Spot)**
- **Source:** internal program critique — entire book is 100% long-biased trend direction; no way to express bearish or defensive view; KuCoin/Bitcoinist bear-market literature (2026) confirms directional shorting remains an active retail/institutional strategy.
- **Edge hypothesis:** A short-side sleeve would give the book a genuine crisis-alpha lane. Options: (1) inverse-BTC futures when BOCPD evidence drops below -1.5 (stressed regime), (2) short high-funding high-OI symbols in leverage-fragility state, (3) cross-asset short (short crypto / long SPX in crypto-leadership regime). The key is using a short signal only when the non-price state strongly predicts near-term drawdown, not as a standalone directional bet.
- **Data requirements:** BTC 1d or 8h returns; Binance funding data; SPX or DXY if cross-asset.
- **Complexity:** medium
- **Novelty vs current work:** very high — this is the single biggest structural gap in the current book; no short-side family exists anywhere in the current HALL_OF_FAME or pipeline.
- **Minimum viable test:** add a simple short sleeve (inverse-BTC when EWMA-CUSUM evidence < -1.0) to the frozen three-sleeve book; compare against long-only baseline on same 9 universes with explicit attention to bear-market windows.
- **Priority:** HIGH
- **Status:** proposed — NEW 2026-04-05 (flagged as highest structural priority gap)

**Leverage-Fragility State with Real Funding Data**
- **Source:** Oct 2025 $19B liquidation cascade (SSRN); Amberdata leverage purge report (Dec 2025) — OI peaked $47.4B, funding 4.5% APR, $5.7B liquidations; Q4 2025 leverage resets reduced fragility; Amberdata identifies leverage fragility states (`R4 Fragile / R5 Overextension / R6 Fragile Recovery`).
- **Edge hypothesis:** The leverage-fragility overlay scaffold exists but uses `btc_volume` as the funding proxy — wrong. Real funding data is in `data/funding_cache/*.parquet`. Build leverage-fragility index from: OI percentile rank (from Binance futures) + funding rate percentile + realized vol z-score. Amberdata's own R4/R5/R6 labels give concrete state thresholds to calibrate against.
- **Data requirements:** Binance futures OI history (public, ~31 rows daily — too shallow for multi-year, but usable for recent windows); Binance 8h funding rates from `data/funding_cache/*.parquet`; BTC realized vol.
- **Complexity:** low-medium
- **Novelty vs current work:** very high — derivatives-state lane; directly uses Oct 2025 cascade mechanics; no new auth needed for funding data.
- **Minimum viable test:** fix funding proxy in existing `leverage_fragility_state_overlay.rs`; rebuild leverage-fragility index; compare post-cascade vs calm next-21-bar sleeve returns; if post-cascade returns are reliably better, use as selective entry augment.
- **Priority:** HIGH
- **Status:** proposed — NEW 2026-04-05 (data is available; only the proxy was wrong)

---
## 2026-04-05 Research Cycle Additions

### Leverage-Fragility Index with Concrete Amberdata Thresholds
- **Source:** Amberdata `$31B Deleveraging` blog (2 weeks ago); Galaxy Q2 2025 leverage report — Oct 2025 cascade mechanics with specific, actionable thresholds.
- **Concrete metrics from Oct 2025 cascade:**
  - OI peaked at $54.7B before collapse; sat 42% below peak post-cascade
  - $31.4B total BTC liquidations YTD — 60% from longs; Oct 10 alone: $2.3B liquidated, 86% forced long closures
  - Funding rates above **15% APR** = danger signal; above **20% APR** = R4/R5 fragile state
  - Liquidation intensity = funding × OI / realized_vol; **>5% of OI** = cascade threshold; Oct reached **4.82%**
- **Edge hypothesis:** crowding is most dangerous when funding > 15% AND OI at historical highs AND realized_vol elevated. This trifecta predicts imminent cascade better than any single factor. Post-cascade reset = exploitable rebound window.
- **Data requirements:** 8h funding rates from `data/funding_cache/*.parquet`; OI daily from Binance public API (31 rows — shallow but usable for recent windows); realized vol from BTC daily. Auth-free.
- **Complexity:** low-medium
- **Novelty vs current work:** very high — derivatives-state lane with concrete validated thresholds from a real $31B event; completely different from price/volume/pressure overlays
- **Minimum viable test:** build daily leverage-fragility index from funding percentile + OI percentile + realized_vol z-score; test whether high-fragility states precede worse 21-bar sleeve returns (de-risk trigger) and post-fragility-calm states precede better returns (rebound augment). Use 15% and 20% APR funding thresholds as anchors.
- **Priority:** HIGH
- **Status:** proposed — NEW 2026-04-05

### Intraday Sub-Daily Mean Reversion (15m–4h bars)
- **Source:** internal program critique — EVERYTHING is daily OHLCV resolution. OFI, VPIN, pressure, observable-state, change-point, relative-strength, BOCPD, EWMA-CUSUM — all daily. All came back near-null or too-centered. The instrument is daily bars, which averages away the short-horizon mean-reversion that literature suggests exists at 15m–4h.
- **Edge hypothesis:** mean reversion at daily bars is a NULL result in crypto because noise averages away. But at 15m–1h bars, short-term supply/demand imbalances create exploitable mean reversion that doesn't survive at daily resolution. December 2025 literature confirms intraday return predictability in crypto.
- **Data requirements:** Binance klines at 15m and 1h resolution. BTC/ETH/SOL min. Data is accessible via Binance public API (existing loader infrastructure).
- **Complexity:** low
- **Novelty vs current work:** extremely high — completely unexplored time horizon; zero sessions have tested sub-daily bars; all prior work is daily resolution
- **Minimum viable test:** 15-bar rolling z-score of log-return on 15m and 1h bars for BTC/ETH; entry when z > 2.0, exit when z < 0; compare next-15-bar return vs baseline (no signal); test across recent 3-month window where 15m data exists. If positive edge, expand to ETH/SOL cross-section.
- **Priority:** HIGH
- **Status:** proposed — NEW 2026-04-05 (zero prior intraday research in entire program history)

### Stablecoin Exchange Reserve State (Binance Public API — No Auth)
- **Source:** CryptoQuant exchange whale ratio; Binance exchange balance history (coin 总仓储); CoinMarketCap research — USDT/USDC exchange balances signal near-term buying/selling pressure. Bitcoin exchange reserves at 7-year lows (March 2026). Binance traders accumulating ETH — net withdrawals from exchanges + rising stablecoin reserves signal potential buying pressure.
- **Edge hypothesis:** USDT/USDC exchange balance changes relative to total stablecoin supply mark whale distribution vs accumulation. Rising stablecoins on exchange = whales preparing buying firepower (bullish). Falling exchange balances + rising cold storage = accumulation (bullish longer-term, near-term selling risk). Concrete signal: `exchange_usdt_balance / 30d_avg_exchange_balance`.
- **Data requirements:** Binance public API — `GET /sapi/v1/asset/currencycanDeposit/address` or aggregated exchange reserve history. Daily USDT + USDC exchange balance. No auth required for basic endpoints.
- **Complexity:** low-medium (API access straightforward; need to find the right endpoint)
- **Novelty vs current work:** very high — completely different non-price data lane; whale flow is orthogonal to all price/volume/derivatives signals tested so far
- **Minimum viable test:** compute rolling 7d/30d exchange balance ratio for USDT on Binance; label `accumulating` (ratio rising) vs `distributing` (ratio falling); test next-7d and next-21d BTC returns conditioned on state. If distribution precedes drawdowns and accumulation precedes rallies, build state into sleeve risk-control.
- **Priority:** HIGH
- **Status:** proposed — NEW 2026-04-05

---
### NEW — 2026-04-07 20:07 UTC (Research & Critique Cycle)

**LOB NOBI Daily Aggregate Signal**
- **Source:** arxiv 2507.22712 "Order Book Filtration and Directional Signal Extraction" (July 2025) + arxiv 2502.18625 "To Make or to Take" (Feb 2025) — ephemeral LOB states degrade directional signals; structural filtering needed. LOB daemon already built (1862b57) but NOBI signal never tested.
- **Edge hypothesis:** Without proper structural filtering, raw NOBI at high frequency is too noisy. The actionable signal requires: (1) accumulate depth snapshots over 1h window, (2) apply SG smoothing, (3) test at daily resolution. Then: high NOBI (buy-side dominant) → next-day directional continuation.
- **Data requirements:** Binance depth endpoint (no auth) — daemon already collecting.
- **Complexity:** low — just run the test on already-collected data.
- **Novelty vs current work:** very high — microstructure lane, not price/volume/derivatives; the DAEMON was built but the SIGNAL was never tested.
- **Minimum viable test:** Take existing LOB snapshot data → compute daily top-5 depth imbalance → SG smooth → test if daily NOBI > threshold predicts next-24h directional return vs zero baseline.
- **Priority:** HIGH — daemon already collecting data, signal test is one harness.
- **Status:** proposed — NEW 2026-04-07

**BTC-ETH Cointegration Pair Trading (Proposed, Never Tested)**
- **Source:** Frontiers Jan 2026 "Deep learning-based pairs trading: real-time forecasting of co-integrated cryptocurrency pairs" — BTC-ETH cointegrating coefficient ~0.0587. Dynamic Johansen method.
- **Edge hypothesis:** First credible mean-reversion object in the entire program. ETH-BTC spread oscillates around stable equilibrium. When it diverges, there's a predictable return path back. Completely orthogonal to all current trend/A/D book.
- **Data requirements:** BTC + ETH daily prices (already cached).
- **Complexity:** medium — needs rolling cointegration coefficients + z-score of spread for entry/exit.
- **Novelty vs current work:** very high — genuine mean-reversion family, first in pipeline. All prior mean-reversion (RSI, BollingerReversion) is GRAVEYARD.
- **Minimum viable test:** compute rolling BTC-ETH cointegration; generate spread z-score; test whether mean-reversion entries improve Sharpe on ETH-containing universes.
- **Priority:** HIGH — first credible mean-reversion idea; completely unexplored.
- **Status:** proposed — NEW 2026-04-07

**ETF Flow Institutional Signal (Data Available, Never Collected)**
- **Source:** CoinGlass (free), Glassnode (free tier), Farside — daily spot BTC ETF net flows publicly available.
- **Edge hypothesis:** Institutional ETF flow is the marginal driver of BTC price in 2025-2026. 5-day rolling net outflow → hard risk-off filter for entire portfolio. CoinGlass has the data on free tier.
- **Data requirements:** CoinGlass free ETF flow page, or Glassnode free tier.
- **Complexity:** low — simple scraping or manual collection of daily net flow numbers.
- **Novelty vs current work:** extremely high — zero institutional flow data anywhere in the program.
- **Minimum viable test:** collect ETF flow timeseries from CoinGlass; build 5-day rolling flow z-score; test if negative flow z-score precedes reliably worse 21-bar sleeve returns.
- **Priority:** HIGH — data exists, collection is trivial, institutional-grade signal.
- **Status:** proposed — NEW 2026-04-07

---

## Ideas Status Summary

| Idea | Priority | Status |
|------|----------|--------|
| LOB Depth Imbalance Daily Aggregate | HIGH | proposed — NEW 2026-04-06 |
| Weekly Order Flow Imbalance Sleeve | HIGH | proposed — NEW 2026-04-06 |
| Extreme Funding State as Trend Entry Augment | HIGH | proposed — NEW 2026-04-06 |
| VPIN / Order-Flow Toxicity Crash-State Overlay | HIGH | proposed — NEW 2025 literature |
| Bayesian Online Changepoint Detection (BOCPD) with Covariates | HIGH | proposed — NEW 2025 literature |
| Cointegration Pair Trading (BTC-ETH spread) | HIGH | proposed — NEW 2026 literature |
| Cross-Asset Spillover / Relative-Strength Allocator | HIGH | proposed |
| Exchange reserve / whale-flow crowding state | HIGH | proposed (auth blocked) |
| Tail-Risk Monte Carlo / Drawdown-Parity Allocator | HIGH | proposed |
| RL-for-Sizing Wrapper on Frozen Sleeves (SAC/DDPG) | MEDIUM | proposed — NEW 2025 literature |
| Weekly aggregated order-flow / slow-pressure factor | HIGH | tested — secondary only |
| Liquidation cascade / leverage-fragility state | HIGH | proposed |
| Connectedness / spillover regime overlay | MEDIUM | proposed |
| Observable-covariate state score | MEDIUM | tested — too neutral in v1 |
| Slow flow / taker-pressure factor | MEDIUM | proposed |
| Intraday OI + funding crowding state study | MEDIUM | exploratory only |
| LOB imbalance / microprice collector | MEDIUM | proposed |
| Cross-asset macro factor overlay | MEDIUM | proposed |
| Risk-managed crypto factor momentum | HIGH | proposed |
| Tail-risk / drawdown-parity portfolio overlay | HIGH | proposed |
| Bayesian/change-point state annotator | HIGH | proposed — now superseded by BOCPD |
| Macro/uncertainty-conditioned crypto factor overlay | HIGH | proposed |
| Drawdown-controlled multi-sleeve allocator with dynamic cash | MEDIUM | proposed |
| Weekly pressure fourth-sleeve / state-attribution audit | MEDIUM | tested — secondary only |
| Crypto cross-sectional factor book (trend + size/liquidity + residual momentum) | HIGH | proposed |
| Crypto Factor Sleeves (size / momentum / value / illiquidity) | HIGH | tested — Small sleeve alive |
| CTREND-style multi-horizon price+volume factor | HIGH | tested — credible trend yardstick |
| A/D + one trend representative under capped-book allocator | HIGH | tested — sleeve allocator works |
| Open interest + funding crowding state layer (daily) | HIGH | infra-blocked |
| A/D symbol-aware allocation | LOW | tested — not a trust winner |
| DDHard / family DD-budget overlay | 🟡 | current allocator baseline |
| Breadth-aware recovery overlay | 🟡 | promising refinement, not baseline |
| HMM regime prototype V2 | 🔴 | GRAVEYARD |
| RSI threshold mean-reversion | 🔴 | GRAVEYARD |
| OFI Momentum (daily OHLCV proxy) | 🔴 | GRAVEYARD |
| OFI Mean-Reversion | 🔴 | GRAVEYARD |
| Simple/beta-neutral funding carry | 🔴 | GRAVEYARD |
| Perp-vs-spot basis carry | 🔴 | GRAVEYARD |
| Relational residual momentum (cluster/graph/eigenfactor) | 🔴 | GRAVEYARD |
| BollingerReversion with realistic execution | 🔴 | GRAVEYARD |

---

## Detailed Ideas — New and Updated

### VPIN / Order-Flow Toxicity Crash-State Overlay
- **Source:** `Bitcoin wild moves: Evidence from order flow toxicity and price jumps` (ScienceDirect, October 2025); VPIN > 0.7 + skewed OBI predicts sharp directional moves; recent practitioner reporting confirms VPIN + OBI combo flags imminent crash risk.
- **Edge hypothesis:** High VPIN (toxic one-sided informed flow) should predict unstable continuation, jump risk, and worse fills. This is more principled than our dead OHLCV OFI proxy because it uses volume-synchronized probability of informed trading rather than simple bar-imbalance. Best use is as a **risk-off / de-leveraging overlay** on the frozen sleeve book, not as raw directional alpha.
- **Data requirements:** trade-side volume buckets or VPIN computation requires tick-level data. Minimum viable proxy: signed volume imbalance over rolling short windows aligned to our existing Binance bars, or collection via a dedicated microstructure scraper.
- **Complexity:** medium-high (data collection is the hard part; computation is simple)
- **Novelty vs current work:** very high — this is a genuine microstructure signal lane, completely different from the dead OFI proxy and from all our price/volume overlays
- **Minimum viable test:** build rolling VPIN buckets (or nearest viable proxy) for BTC/ETH; test whether high-toxicity states precede worse next-21-bar sleeve outcomes on the frozen three-sleeve book; if correlation holds, add as a selective de-risking trigger
- **Priority:** HIGH
- **Status:** proposed

### Bayesian Online Changepoint Detection (BOCPD) with Observable Covariates
- **Source:** `Bayesian Online Changepoint Detection for Financial Time Series` (ACM 2025); `Bayesian Change Point Detection in Financial Time Series` (ACM 2025); `Improving S&P 500 Volatility Forecasting through Regime-Switching Methods` (arXiv 2025). Recent BOCPD literature shows it detects COVID/monetary policy regimes on S&P 500 and CSI 300. Embedding covariates (vol stress, breadth, correlation concentration) improves specificity vs our blunt BTC-only shock guard.
- **Edge hypothesis:** BOCPD is better than our failed HMM because it does not assume a fixed number of latent states — it detects changes in run-length directly. Adding observable covariates (breadth collapse, vol shock, correlation break, optional OI/reserve) gives it more signal than our near-null v1 observable state score which stayed ~99.7% neutral.
- **Data requirements:** daily returns, plus existing breadth/vol/correlation features from the frozen sleeves
- **Complexity:** medium
- **Novelty vs current work:** high — replaces the blunt BTC-only change-point guard and the too-neutral observable state v1 with a principled online detector
- **Minimum viable test:** run BOCPD on sleeve returns with breadth + vol + correlation covariates; generate discrete break flags; use as family-weight or cash tilt context only; benchmark against plain DDHard and the failed simple change-point overlay
- **Priority:** HIGH
- **Status:** proposed

### Cointegration Pair Trading (BTC-ETH Spread)
- **Source:** `Deep learning-based pairs trading: real-time forecasting of co-integrated cryptocurrency pairs` (Frontiers, January 2026); `Statistical Arbitrage Strategies Using Cointegration` (IJSRA 2026) — dynamic Johansen cointegration method for BTC-ETH pair. BTC-ETH cointegrating coefficient ~0.0587, meaning ETH trades at ~5.87% of BTC in long-run equilibrium.
- **Edge hypothesis:** cointegration identifies genuine equilibrium relationships where the spread is mean-reverting, unlike simple correlation which just measures co-movement. ETH-BTC spread should oscillate around a stable equilibrium — when it diverges, there's a predictable return path. This is the first credible mean-reversion / non-trend idea we've found. Completely orthogonal to our current trend + A/D book.
- **Data requirements:** daily BTC/ETH prices, rolling cointegration coefficients (Engle-Granger or Johansen), z-score of spread for entry/exit signals
- **Complexity:** medium
- **Novelty vs current work:** very high — this is a genuine mean-reversion family, the only one in our research pipeline. All other "mean-reversion" attempts (RSI thresholds, BollingerReversion) are already dead.
- **Minimum viable test:** compute rolling BTC-ETH cointegration; generate spread z-score signals; test whether mean-reversion entries improve Sharpe or reduce DD on ETH-containing universes; compare against the no-pair baseline
- **Priority:** HIGH
- **Status:** proposed

### Cross-Asset Spillover / Relative-Strength Allocator
- **Source:** `From Disruption to Integration: Cryptocurrency Prices, Financial Fluctuations, and Macroeconomy` (MDPI 2025) — crypto price shocks explain 18% of equity price fluctuations and 27% of commodity fluctuations; Bitcoin increasingly treated as safe-haven vs tech stocks during trade policy disputes; `Trading Games: Beating Passive Strategies in the Bullish Crypto Market` (Wiley 2025) — cointegration-based pair trading adapting to market conditions.
- **Edge hypothesis:** the useful cross-asset object is not a generic macro stress gate — it is the directional **spillover relationship**: crypto leads equity in some regimes, lags in others. Capturing this leadership/lagging with a rolling `crypto / equity` or `crypto / DXY` relative-strength signal could improve the timing of our existing sleeve book without the blunt exposure-suppression problem of our prior macro overlay.
- **Data requirements:** existing SPX and DXY history already cached; no new auth needed
- **Complexity:** low-medium
- **Novelty vs current work:** high — different from our prior generic macro-stress overlay which was too blunt; this focuses on leadership identification rather than binary risk-on/off
- **Minimum viable test:** build rolling `BTC/SPX` and `ETH/SPX` relative-strength z-scores; use as selective sleeve-weight modifier only when leadership is statistically significant; benchmark against plain frozen baseline and the prior failed generic macro overlay
- **Priority:** HIGH
- **Status:** proposed

### Tail-Risk Monte Carlo / Drawdown-Parity Sleeve Allocator
- **Source:** `Quantifying Crypto Portfolio Risk: A Simulation-Based Framework Integrating Volatility, Hedging, Contagion, and Monte Carlo Modeling` (arXiv 2025); `Optimising cryptocurrency portfolios through stable clustering of price correlation networks` (arXiv 2025) — partial correlation network approach for systemic tail dependence; risk parity based on risk contribution rather than expected returns.
- **Edge hypothesis:** our current DDHard allocator throttles by hard DD thresholds. A more principled approach is to allocate sleeve capital by each sleeve's **expected shortfall contribution** or **drawdown-parity** rather than equal risk budgets. This is a richer portfolio construction idea than fixed DD tiers.
- **Data requirements:** existing sleeve return streams; Monte Carlo simulation over the sleeve universe
- **Complexity:** medium
- **Novelty vs current work:** medium — extends DDHard rather than replacing it; directly addresses the concentration/crowding findings from the overlap audit
- **Minimum viable test:** compute rolling ES contributions per sleeve; allocate sleeve risk by ES contribution rather than equal DDHard tiers; compare against plain DDHard on the same 9 universes
- **Priority:** HIGH
- **Status:** proposed

### Exchange Reserve / Whale-Flow Crowding State
- **Source:** on-chain practitioner literature; CryptoQuant / Whale Alert / Arkham ecosystem; extreme positive funding historically precedes corrections; extreme negative funding precedes short squeezes; whale deposit stress flags near-term sell pressure.
- **Edge hypothesis:** exchange inflow spikes and whale-deposit concentration mark near-term sell pressure; reserve drawdowns and outflows confirm accumulation. Works better as **risk-control / state feature** than as raw directional alpha.
- **Data requirements:** exchange reserves, netflow, whale deposit ratios — requires COINGLASS_API_KEY or similar
- **Complexity:** medium
- **Novelty vs current work:** very high — genuinely different from price/volume/derivatives
- **Minimum viable test:** build BTC daily state labels (`reserve_rising_sell_pressure`, `reserve_falling_accumulation`, `whale_deposit_stress`, `neutral`); compare next-21-bar forward returns; test whether states should de-risk the frozen sleeve book
- **Priority:** HIGH
- **Status:** proposed (auth blocked)

### RL-for-Sizing Wrapper on Frozen Sleeves (SAC/DDPG)
- **Source:** `Reinforcement Learning-Based Cryptocurrency Portfolio Management Using Soft Actor–Critic and Deep Deterministic Policy Gradient Algorithms` (arXiv 2025); `Smart Tangency Portfolio: Deep Reinforcement Learning for Dynamic Rebalancing and Risk–Return Trade-Off` (MDPI 2025); RL shows steady improvement in crypto applications per 2025 systematic review.
- **Edge hypothesis:** RL is dangerous for signal discovery but potentially useful as a constrained **sizing wrapper** over already-frozen sleeves. If action space is limited to discrete exposure levels (0.25/0.5/0.75/1.0) and penalties are explicit (turnover cost, drawdown penalty, Sharpe downside), it may learn when to lean up/down without inventing fake alpha.
- **Data requirements:** sleeve return streams, observable state inputs, strict train/validation/test chronology split
- **Complexity:** high
- **Novelty vs current work:** medium-high — different from all our rule-based allocators
- **Minimum viable test:** implement SAC agent choosing only discrete exposure levels for A/D, MACD, Small, cash; heavy regularization and turnover penalties; compare against DDHard and simple rule overlays out-of-sample on the same 9 universes. CRITICAL: do not let RL pick assets or generate signals.
- **Priority:** MEDIUM
- **Status:** proposed

### Liquidation Cascade / Leverage-Fragility State
- **Source:** `Anatomy of the Oct 10–11, 2025 Crypto Liquidation Cascade` (SSRN, October 2025) — $19B OI erased in 36 hours, triggered by macro announcement. Key mechanics: leverage concentration + fragile liquidity + automated risk management created mechanical feedback loop. Cascade resets OI from speculative highs, creates exploitable post-cascade windows.
- **Edge hypothesis:** crowded leverage is most dangerous when OI is near historical highs AND funding is extreme AND realized vol is spiking simultaneously. This trifecta predicts imminent liquidation cascade better than any single factor. Post-cascade, the reset creates mean-reversion opportunity as forced positions clear.
- **Data requirements:** OI levels, funding rates, realized vol — all accessible from Binance public API. No auth needed. Build a daily leverage-fragility index from: OI percentile rank + funding rate percentile + realized vol z-score.
- **Complexity:** low-medium
- **Novelty vs current work:** very high — derivatives-state lane; actionable mechanics from Oct 2025 event study; completely different from price/volume overlays
- **Minimum viable test:** build daily leverage-fragility index; label `pre-cascade-stress` / `post-cascade-reset` / `calm` states; test whether the frozen three-sleeve book has better or worse next-21-bar returns post-cascade vs calm; if post-cascade returns are reliably better, use as selective entry augment rather than de-risk trigger
- **Priority:** HIGH
- **Status:** proposed — NEW 2026-04-04 (upgraded from older liquidation idea with concrete Oct 2025 mechanics)

### Whale-Flow / Stablecoin-Exchange-Balance State
- **Source:** Nansen (September 2025) + Bitget research — stablecoin balances on exchanges signal ready liquidity to buy dips; large stablecoin transfers onto exchanges precede significant crypto purchases; whale deposit concentration marks sell pressure. whale accumulation phases show: sustained net inflows to non-exchange addresses, declining exchange reserves, increased tx counts to cold storage.
- **Edge hypothesis:** USDT/USDC exchange balances are a practical whale-flow proxy without requiring full on-chain auth. Rising stablecoin exchange balances = whales preparing firepower for buying (bullish). Declining stablecoin exchange balances = whales accumulating off-exchange (bullish longer-term but near-term selling risk). The key insight is this is tractable without CoinGecko-level auth.
- **Data requirements:** USDT + USDC exchange balances — accessible via public Binance API (coin總仓储) or CoinGecko exchange metrics. Daily USDT + USDC exchange reserve change rate as primary signal.
- **Complexity:** low-medium
- **Novelty vs current work:** high — stablecoin flow is genuinely different from price/volume/derivatives; tractable with public API; not the same as full on-chain whale tracking
- **Minimum viable test:** build daily stablecoin-exchange-reserve-change state (`accumulating-off-exchange`, `distributing-to-exchange`, `neutral`); compare next-7d and next-21d sleeve returns conditioned on state; if distribution-to-exchange state precedes worse sleeve returns, use as risk-control feature; if accumulation precedes better returns, use as opportunistic entry augment
- **Priority:** HIGH
- **Status:** proposed — NEW 2026-04-04 (practical stablecoin proxy, no auth needed)

### PCA-Based Multi-Regime State Annotator
- **Source:** `Applying reinforcement learning in Bitcoin trading to select technical strategies based on Deep Q-Network` (March 2025) — uses PCA on technical indicators to construct market states, then uses DQN to select strategy accordingly. PCA-based states are more robust than single-threshold BTC-only guards.
- **Edge hypothesis:** our failed observable-state v1 and change-point overlay both suffered from too-blunt feature construction. PCA on a basket of normalized technical indicators (BTC realized vol, BTC trend strength, A/D breadth, cross-asset correlation concentration, small-cap relative strength) should create more discriminative regime labels than any single feature threshold.
- **Data requirements:** existing daily features from the frozen sleeves + BTC vol + correlation features — no new data needed
- **Complexity:** medium
- **Novelty vs current work:** high — more principled feature construction than our blunt BTC-only vol shock guards; directly addresses why observable-state v1 stayed 99.7% neutral
- **Minimum viable test:** build PCA on 5-7 normalized regime indicators; use top 2-3 PCs to label regimes; compare regime-conditioned sleeve returns against unconditioned baseline on the same 9 universes; if PC-based regimes have better state dispersion than our prior attempts, promote as the state annotator for overlay work
- **Priority:** HIGH
- **Status:** proposed — NEW 2026-04-04 (addresses specific failure mode of observable-state v1)

### Liquidation Cascade / Leverage-Fragility State (original)
- **Source:** 2025 liquidation-cascade literature; market-structure reporting; VPIN crash prediction literature.
- **Edge hypothesis:** crowded leverage is only dangerous when liquidation pressure is near trigger levels. Liquidation clusters predict short-horizon instability, forced continuation, and reflexive reversal windows better than funding alone.
- **Data requirements:** liquidation notional/counts by symbol and side — likely requires Amberdata/CoinGlass/Hyblock; BTC/ETH/SOL first
- **Complexity:** medium
- **Novelty vs current work:** very high — derivatives-state lane rather than another price overlay
- **Minimum viable test:** build coarse daily/4h states (`fragile-long`, `fragile-short`, `post-cascade-reset`, `calm`); test whether these states should de-risk the frozen sleeve book or selectively allow rebound trades after cascades
- **Priority:** HIGH
- **Status:** proposed — superseded by concrete Oct 2025 cascade paper (see above)

### Connectedness / Spillover Regime Overlay
- **Source:** factor audit showing one broad trend basin plus A/D; partial correlation network approach for systemic tail dependence (arXiv 2025).
- **Edge hypothesis:** the relevant regime may be market coupling, not just BTC bull/bear. When cross-asset connectedness is high, trend sleeves behave like one beta engine and deserve tighter budgets; when connectedness fragments, A/D or narrower books add more value.
- **Data requirements:** existing daily return panel across current universes
- **Complexity:** medium
- **Novelty vs current work:** high — addresses real structural issue
- **Minimum viable test:** compute rolling connectedness proxy (correlation concentration, first-PC explained variance); use it to tilt sleeve budgets; benchmark against frozen `DDBudget(A/D,MACD,Small)` baseline
- **Priority:** MEDIUM
- **Status:** proposed

### Slow Flow / Taker-Pressure Factor
- **Source:** EFMA 2025 `Order Flow and Cryptocurrency Returns`; weekly aggregation mitigates microstructure noise.
- **Edge hypothesis:** true trade pressure likely works at intermediate horizons. A slow taker-buy imbalance or signed-dollar-flow factor could be the first credible flow family.
- **Data requirements:** taker-buy base/quote volume by bar, or trade-side imbalance; BTC/ETH/SOL/XRP first
- **Complexity:** medium
- **Novelty vs current work:** high — much cleaner than dead OFI proxy
- **Minimum viable test:** build 5d/7d/14d signed-pressure ranks; compare against `A/D`, `CTREND`, `WeeklyPressureAccel` under same chronology-first harness
- **Priority:** HIGH
- **Status:** proposed

### Intraday OI + Funding Crowding State Study
- **Source:** internal OI depth audit; derivatives-state literature.
- **Edge hypothesis:** public Binance OI is too shallow for daily multi-year research, but still useful for short-horizon crowding studies. Edge is not carry; it is identifying unstable crowded continuation states.
- **Data requirements:** Binance 1h OI history (~20.8 days currently available), aligned funding, BTC/ETH/SOL first
- **Complexity:** medium
- **Novelty vs current work:** high, but sample-depth-limited
- **Minimum viable test:** label hourly states (`healthy participation`, `weak rally`, `crowded long`, `crowded short`); measure next 24h/72h returns; test short-horizon de-risk overlay
- **Priority:** HIGH
- **Status:** proposed

### Regime/State Detector with Observable Covariates
- **Source:** BOCPD literature (now primary reference); replaces older HMM/covariate approach.
- **Edge hypothesis:** short-sample latent HMMs are the wrong tool. BOCPD with explicit covariate signals (breadth, vol, correlation, reserve if available) should outperform blunt BTC-only guards.
- **Data requirements:** existing sleeve features; optional reserve, funding, OI later
- **Complexity:** medium
- **Novelty vs current work:** high
- **Minimum viable test:** implement BOCPD with covariates; benchmark against DDHard and failed simple change-point overlay
- **Priority:** HIGH
- **Status:** proposed — supersedes older "observable state score" idea

### LOB Depth Imbalance Daily Aggregate (NEW — 2026-04-06)
- **Source:** Wang, "Exploring Microstructural Dynamics in Cryptocurrency Limit Order Books" (arxiv 2506.05764, June 2025) — Best Paper runner-up; empirical on Bybit BTC/USDT 100ms LOB data
- **Edge hypothesis:** the dead OHLCV-based OFI proxy failed because it used bar-summary statistics as a microstructure proxy. The real signal is in LOB depth imbalance (bid vs ask quantity at top 5 levels), aggregated and smoothed with Savitzky-Golay or Kalman filtering. This survives to daily resolution better than raw volume imbalance because it captures supply/demand asymmetry, not just trading activity. Key finding from the paper: simpler models with good preprocessing match deep architectures — preprocessing matters more than model complexity.
- **Data requirements:** Binance public depth endpoint (`/api/v3/depth` — top 20 levels, no auth); aggregate to daily 5-level cumulative imbalance; apply SG smoothing; BTC/ETH/SOL first
- **Complexity:** low-medium (data collection + simple aggregation; no new auth)
- **Novelty vs current work:** very high — properly-constructed microstructure proxy; completely different from the dead OHLCV-based OFI attempt; this is the right instrument at the right resolution
- **Minimum viable test:** build rolling daily 5-level depth imbalance for BTC; smooth with 5-day SG filter; test whether high imbalance states precede directional continuation vs frozen three-sleeve baseline; if positive, extend to ETH/SOL cross-section
- **Priority:** HIGH — lowest lift of any microstructure idea; directly fixes the OFI failure with a principled approach; no new auth needed
- **Status:** proposed — NEW 2026-04-06

### Weekly Order Flow Imbalance Sleeve (NEW — 2026-04-06)
- **Source:** Anastasopoulos & Gradojevic, "Order Flow and Cryptocurrency Returns" (EFMA 2025); consistent with arxiv 2506.05764 preprocessing insight
- **Edge hypothesis:** the optimal aggregation horizon for order flow is WEEKLY, not daily. Daily flow is too noisy from microstructure flicker. Weekly signed-dollar-flow imbalance (computed from our existing Binance bar data) should predict 1–4 week returns after noise averaging. This is a different object from our weekly pressure factor (which was signed-volume, not dollar-volume weighted).
- **Data requirements:** daily BTC/ETH/SOL close + volume from existing Binance loader; re-aggregate to weekly; no new source needed
- **Complexity:** low — just re-aggregates existing data at weekly horizon; dollar-volume weighting is the only difference from existing weekly pressure
- **Novelty vs current work:** high — genuinely different time horizon AND different from the weekly pressure factor (dollar vs volume weighting); zero prior weekly flow research
- **Minimum viable test:** compute weekly signed-dollar-flow for BTC/ETH/SOL; rank symbols by weekly flow direction; test whether flow-ranked next-week returns are predictable; if yes, add as a weekly rebalance sleeve for the frozen three-sleeve book
- **Priority:** HIGH — genuinely different time horizon; no new data collection; directly complementary to existing daily sleeve book
- **Status:** proposed — NEW 2026-04-06

### Extreme Funding State as Trend Entry Augment (NEW — 2026-04-06)
- **Source:** CoinDesk market data (April 2–6 2026); perp funding most negative since March 12; $400M liquidations; $19B OI unwound; MDPI Jan 2026 "Two-Tiered Structure of Cryptocurrency Funding Rate Markets"
- **Edge hypothesis:** the lever-long purge that drove BTC from $126k → $67k has pushed funding to extreme negative levels. Historical pattern from the Oct 2025 cascade: extremely negative funding (= lever-long position unwind complete) creates cleaner trend-following entry windows than crowded long states (= high positive funding, pre-cascade). Perp-perp carry as a standalone strategy is dead because institutional arb enforces the 0.01%/8h anchor. But funding extreme AS CONTEXT for trend timing is a different object — it tells you when the crowded smart money has already been cleared out.
- **Concrete threshold:** when BTC 8h funding < -3 bps/8h (= most negative since March 12), trend signals get better entry quality because the crowded long smart money has been liquidated
- **Data requirements:** 8h funding from `data/funding_cache/*.parquet` — already cached; no new auth needed
- **Complexity:** low
- **Novelty vs current work:** medium — reframes funding as trend-entry context rather than carry signal; not the same as the dead perp-perp carry thesis
- **Minimum viable test:** label days by funding percentile; compare next-21-bar sleeve returns conditioned on extreme-negative funding state vs baseline across same 9 universes
- **Priority:** HIGH — directly actionable with cached data; addresses the trend entry timing question; genuinely different from the dead carry thesis
- **Status:** proposed — NEW 2026-04-06

---

## Research Program Meta-Analysis — 2026-04-04 23:56 UTC (Kira, new research sweep)

### New literature findings this cycle
1. **Oct 2025 $19B liquidation cascade** (SSRN, October 16, 2025) — `$19B OI erased in 36 hours`, triggered by macro announcement, driven by mechanical leverage cascade. Key insight: cascade resets OI from speculative highs, creates post-cascade mean-reversion windows. This is actionable as a **leverage-fragility state** with concrete mechanics.
2. **VPIN + OBI combo for directional crash prediction** (ScienceDirect, October 2025) — VPIN > 0.7 + skewed order book imbalance predicts sharp directional BTC moves. More actionable than our failed daily OHLCV proxy.
3. **Multi-horizon PCA state construction** (March 2025 DQN paper) — PCA on technical indicators creates market states that beat simple thresholds. More principled than our blunt BTC-only shock guards.
4. **Whale-flow practical signals** (Nansen, Bitget, 2025-2026) — stablecoin exchange balances precede crypto purchases; whale deposit concentration marks sell pressure. More tractable than full on-chain auth.
5. **Factor investing in crypto systematic review** (SSRN, April 2025) — cross-exchange arbitrage, factor-based strategies survive in crypto. Momentum and size factors most robust.
6. **Deep RL crypto futures** (MDPI August 2025) — timeframe analysis + RL for crypto futures portfolio. Reinforcement learning shows steady improvement in crypto applications per 2025 systematic review.

### Honest critique of recent runs (post-19:37 UTC refresh)
1. **We are not parameter fishing — we are neighborhood fishing.** Last 20+ commits all in the same `A/D + trend + Small + pressure/state overlay` basin. Disciplined but increasingly local.
2. **BTC/ETH cointegration was the best new idea.** First alive mean-reversion object. But fragility tells us we need regime attribution before expansion — we don't yet understand *when* the pair works.
3. **VPIN crash-state was the right lane, wrong instrument.** Daily OHLCV toxicity proxy was too crude. VPIN needs volume buckets or trade-side data, not OHLCV bar proxies.
4. **TailParity audit found something real but possibly just momentum amplification.** Equal-weight three-sleeve is the honest win — diversification helps Sharpe in hostile universes. TailParity uncapped may just be allocating more to what worked recently.
5. **The three-sleeve book is structurally crowded on hostile baskets.** The overlap/concentration audit confirmed this. We are not running truly independent multi-family diversification.
6. **Biggest single blind spot:** still no short-side / bear market alpha. The entire book is long-biased trend direction.

### Most promising genuinely new ideas (updated)
1. **Liquidation cascade / leverage-fragility state** — Oct 2025 $19B cascade paper gives concrete mechanics. Build leverage-fragility index from OI spikes + funding rate extremes + realized vol shock. Use as post-cascade rebound entry signal or pre-cascade de-risk trigger.
2. **BOCPD with covariates** — 2025 ACM papers show BOCPD detects COVID/monetary regimes on S&P 500 and CSI 300. Better than our too-neutral observable state v1 and failed HMM. Uses run-length directly, not fixed latent states.
3. **Whale-flow stablecoin proxy** — stablecoin exchange balances (USDT/USDC flows to exchanges) signal near-term buying pressure. No API auth needed — can proxy with exchange-level flow estimates from available data.
4. **PCA-based multi-regime state** — Use PCA on a basket of technical indicators to construct regime states rather than single-threshold BTC vol guards. More principled than blunt BTC-only shock.
5. **Factor momentum residual alpha** — After A/D + trend + Small, the residual cross-sectional spread in our universe is unexploited. A residual momentum sleeve (long past winners / short past losers within the signal set) could be genuinely orthogonal.

### Biggest blind spot
- Still **no short-side or crisis-alpha family**. The book is 100% long-biased trend direction with no way to express a bearish or defensive view.
- Still **no credible non-price state lane** with enough depth to stand beside A/D and trend.
- **Allocator gains may still be mostly exposure shrinkage in disguise.**

### If deploying real money tomorrow
- Only a **small, throttled, cash-aware** version of the frozen sleeve book.
- **Not** the overlay stack, pressure promotion, or any claim of full diversification.
- **Not** the cointegration pair without regime attribution first.

### What a skeptical fund quant would say
- You have one broad trend engine plus partial A/D diversifier. The allocator is more sophisticated than the alpha.
- The three-sleeve book is crowded and more correlated than the Sharpe table implies.
- Biggest miss: no short-side, no genuine microstructure data, no validated crisis-alpha.
- Next honest move: open a new information lane (liquidation state, BOCPD, or stablecoin-flow proxy), not another local sleeve variant.

---

## Research Program Meta-Analysis — 2026-04-04 15:36 UTC

### Honest critique of the recent loop
1. **The project has been disciplined and honest, but increasingly local.** Last 15+ commits are all variations on `A/D + trend + Small + pressure/state overlays`. Good science, but increasingly packaging the same edge.
2. **Three-sleeve promotion was a portfolio-construction result, not a new alpha discovery.** The gain is diversification and DD-shaping, not a fresh economic premium. The overlap concentration audit confirmed the sleeves are more crowded than the Sharpe tables suggest.
3. **Pressure research was handled honestly but kept us in the same neighborhood.** Weekly pressure survived as a secondary family, but failed both fourth-sleeve promotion and selective-bear-state promotion. We spent too many sessions confirming this.
4. **Macro overlays improved state dispersion but still mostly bought cleaner DD by suppressing exposure.** The relative-strength state was better than generic macro, but the overlay rule was still too blunt.
5. **Observable-state v1 and change-point overlay v1 both came back near-null.** The state maps were too centered/blunt to matter.
6. **Our biggest blind spot is still non-price state information.** We have no credible crisis-alpha family, no mean-reversion family, and no genuine microstructure edge.
7. **Allocator sophistication is now ahead of alpha discovery.** The shell is improving faster than the underlying edge set. A skeptical fund quant would call this out immediately.

### Most promising genuinely new ideas
1. **VPIN crash-state overlay** — October 2025 paper confirms VPIN predicts BTC price jumps. Microstructure lane, completely different from dead OFI proxy. Tractable with volume-imbalance proxies.
2. **Bayesian Online Changepoint Detection (BOCPD)** — 2025 literature shows BOCPD detects regimes on S&P 500 and CSI 300 through COVID/monetary events. Better than our failed HMM and blunt BTC-only guard.
3. **Cointegration pair trading (BTC-ETH spread)** — January 2026 paper on dynamic Johansen cointegration for BTC-ETH. First credible mean-reversion idea. Completely orthogonal to our trend/A/D book.

### Biggest blind spot
- We still do **not** have a convincing non-price state lane with enough depth to stand beside A/D and trend.
- We still have **no mean-reversion or short-side family**, making the book entirely trend-direction long.
- We have **no validated crisis-alpha** — the book has no way to express a bearish or defensive view.
- The biggest program risk is that allocator gains are **mostly exposure shrinkage in disguise**.

### If we had to deploy real money tomorrow
- Only a **small, throttled, cash-aware version** of the frozen sleeve book.
- **Not** the overlay stack, pressure promotion stories, or any claim of a diversified multi-family book.
- Treat results as **research-grade, not deployment-grade**.

### What a skeptical fund quant would say
- Good: you stopped lying to yourself about diversification and execution.
- Bad: you still have one broad trend engine plus a partial A/D diversifier. Allocator is ahead of alpha.
- Missing: short-side/crisis alpha, deeper state data, and a cleaner proof that allocator gains aren't just exposure shrinkage.
- Next honest move: open a new information lane (VPIN/microstructure, BOCPD regime detection, or cointegration mean-reversion), not another local sleeve variant.

## 2026-04-04 19:37 UTC — Critique Refresh After VPIN + Pair First Passes

### Honest critique of the last 5 implementation runs
1. **`crypto_macro_relative_strength_overlay`**
   - Hypothesis quality: acceptable, but still too close to the old macro-gating family.
   - Main flaw: the state had decent dispersion, but the overlay rule mostly just cut exposure. That means the “improvement” risk was mostly risk-shrinkage, not better timing.
   - Takeaway: the state object may be real, but the implementation framing was too blunt.
2. **`change_point_state_overlay` and `observable_state_score_overlay`**
   - Hypothesis quality: directionally good, but under-specified.
   - Main flaw: both state maps were ~all-neutral, which means the feature construction was too centered to express actual market structure changes.
   - Takeaway: this was not parameter fishing, but it did show that we were trying to squeeze too much from weak daily summary features.
3. **`vpin_crash_state_overlay`**
   - Hypothesis quality: strong and genuinely new.
   - Main flaw: the implementation instrument was wrong. A daily OHLCV toxicity stand-in is too crude for a concept whose whole point is volume-time microstructure.
   - Takeaway: good choice of lane, wrong resolution. Continue only with better construction, not local calibration.
4. **`three_sleeve_overlap_concentration_audit`**
   - Hypothesis quality: strong and necessary.
   - Main flaw: none methodologically; this was a good structural trust check.
   - Takeaway: it confirmed the book is not fake-diversified, but also not truly independent. Useful reality check.
5. **`btc_eth_cointegration_pair_benchmark`**
   - Hypothesis quality: strongest of the group because it finally opens an orthogonal mean-reversion lane.
   - Main flaw: current implementation is still fragile to regime dependence and quarter-window instability. Clean resamples alone are not enough.
   - Takeaway: alive, but not promotable. Needs diagnostics before expansion.

### Research-program conclusion from this refresh
- We are **not** obviously parameter fishing, but we **are** spending too much time in a local optimum: refining state overlays around the frozen three-sleeve book.
- The best recent work was the work that either **tested structural trust directly** (`overlap_concentration_audit`) or **opened a genuinely different family** (`cointegration_pair_benchmark`).
- The weakest recent work was any experiment where a “win” could only come from **exposure suppression**.
- Next implementation cycles should prefer:
  1. **BOCPD with covariates** as a more principled state detector than the dead near-neutral maps
  2. **cointegration diagnostics / selective ETH-universe integration**
  3. **true microstructure data collection** (LOB / VPIN / signed-flow buckets), not more daily toxicity proxies
### NEW — 2026-04-07 12:38 UTC (Midday Research & Critique — 2 New Ideas)

**Net Order Book Imbalance (NOBI) Microstructure Signal**
- **Source:** arXiv 2602.00776 / 2506.05764 (2025-2026 LOB dynamics in crypto)
- **Edge hypothesis:** The difference between resting buy liquidity and sell liquidity at the top 5-10 levels of the limit order book directly predicts short-term directional price pressure before trades execute. Consistent cross-asset patterns show that when sell-side liquidity outweighs buy-side liquidity, downward price movement follows.
- **Data requirements:** Binance Depth API (Order Book Snapshots). Need bid/ask volumes at top N levels.
- **Complexity:** medium-high — requires real-time or historical LOB depth data, which is high-frequency.
- **Novelty vs current work:** completely new. We have no LOB resting liquidity signals.
- **Minimum viable test:** Fetch 1-minute LOB snapshots for BTC and ETH. Calculate `(Bids - Asks) / (Bids + Asks)` for top 10 levels. Test if NOBI > X predicts positive 5m-15m returns.
- **Priority:** HIGH — addresses our massive microstructure blind spot.
- **Status:** proposed

**Options Implied Volatility Skew as Regime Risk Filter**
- **Source:** VanEck 2026 Bitcoin ChainCheck / LiveVolatile Feb 2026
- **Edge hypothesis:** When the Implied Volatility (IV) of out-of-the-money puts trades significantly higher than OTM calls and realized volatility (e.g., 25-delta put IV minus 25-delta call IV > 10 points), it indicates institutions are aggressively buying crash protection. A steep volatility skew precedes major drawdowns. We can use this options market pricing to gate our spot trend strategies.
- **Data requirements:** Daily BTC IV surface data (Deribit or similar), specifically 25-delta put and call IVs.
- **Complexity:** medium — requires external options data.
- **Novelty vs current work:** completely new. Brings forward-looking derivatives pricing into our state-space, instead of backward-looking price/volume.
- **Minimum viable test:** Fetch historical daily 25-delta put/call IV for BTC. Calculate IV Skew. Check if Skew > threshold successfully filters out false trend entries before drawdowns better than our 200 SMA or price-based regime filters.
- **Priority:** HIGH — true cross-market regime filter.
- **Status:** proposed

