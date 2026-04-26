//! Live trading bot — Turtle+Chandelier breakout strategy.
//!
//! Signal: Turtle breakout — close > max_close (EP bars lookback)
//! Exit: Chandelier ATR trailing stop OR Turtle ATR stop OR HOLD_MAX reached.
//! Dual exit: whichever stop fires first.
//!
//! Production params (frozen 2026-04-26):
//!   EP=21, CHAND(7, 2.30), ATR(24, 2.0), ATR_ENTRY_MULT=0.00, HM=12, CAP=3
//!   See HALL_OF_FAME.md and PLAN.md for full documentation.
//!
//! Freshness Filter: DISABLED (cd=0) — counterproductive with current tight Chandelier.

const FRESHNESS_COOLDOWN: usize = 0; // bars to wait after exit before re-entry (0=disabled)

// ATR entry multiplier — hyperopt 2026-04-21: EM=0.90 wins full 63-window validation.
// Only enter if close >= max_close + ATR(24) * 0.90 (momentum confirmation filter).
// Prior 6-window sweep found EM=1.0 winner — insufficient validation.
// See memory/hyperopt-2026-04-21-atr-entry-mult-full.md.
const ATR_ENTRY_MULT: f64 = 0.00; // REVERTED 2026-04-25: confirmed 0.00 wins definitive sweep. No entry-side ATR filter.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::collections::VecDeque;
use tokio::sync::RwLock;
use std::sync::Arc;

use super::config::LiveConfig;
use super::executor::{Executor, OrderSide};
use std::path::PathBuf;
use super::feed::{fetch_warmup_data, KlineEvent, LiveFeed};
use crate::paper::{Bar, CompletedTrade, Position};

// =============================================================================
// Per-symbol strategy state (mirrors live_turtle_chandelier.rs)
// =============================================================================
#[derive(Debug, Clone)]
struct TurtleState {
    highest_high: f64,
    lowest_low: f64,
    bars_held: usize,
    atr_buf: VecDeque<f64>,
}

// =============================================================================
// Bot state for monitoring
// =============================================================================
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotState {
    pub started_at: Option<DateTime<Utc>>,
    pub equity: f64,
    pub initial_capital: f64,
    pub total_pnl: f64,
    pub total_return_pct: f64,
    pub trades: usize,
    pub win_rate: f64,
    pub positions: HashMap<String, PositionInfo>,
    pub is_running: bool,
    pub last_bar_time: HashMap<String, DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PositionInfo {
    pub symbol: String,
    pub side: String,
    pub size: f64,
    pub entry_price: f64,
    pub entry_time: DateTime<Utc>,
    pub highest: f64,
    pub lowest: f64,
}

// =============================================================================
// Live trading bot
// =============================================================================
pub struct LiveBot {
    config: LiveConfig,
    feed: Option<LiveFeed>,
    executor: std::sync::Mutex<Executor>,
    state: Arc<RwLock<BotState>>,
    // Per-symbol data
    bars: HashMap<String, Vec<Bar>>,
    positions: HashMap<String, Position>,
    turtle_state: HashMap<String, TurtleState>,
    // Freshness filter: bar index of last exit per symbol
    last_exit_bar: HashMap<String, usize>,
    // Trade tracking
    completed_trades: Vec<CompletedTrade>,
    gross_profit: f64,
    gross_loss: f64,
}

impl LiveBot {
    pub fn new(config: LiveConfig) -> Result<Self> {
        config.validate()?;

        let mut executor = Executor::new(
            config.api_key.clone(),
            config.api_secret.clone(),
            config.dry_run,
            config.use_testnet,
            config.fee_pct,
        );

        // Enable slippage logging to logs/slippage_YYYY-MM-DD.csv
        // Works in both dry_run (simulated fills) and live (real fills)
        let log_date = Utc::now().format("%Y-%m-%d").to_string();
        let log_path = PathBuf::from("logs").join(format!("slippage_{}.csv", log_date));
        if let Err(e) = executor.enable_fill_log(log_path.clone()) {
            tracing::warn!("Failed to enable fill log at {:?}: {}", log_path, e);
        } else {
            tracing::info!("Fill logging enabled: {:?}", log_path);
        }

        let state = Arc::new(RwLock::new(BotState {
            started_at: None,
            equity: config.initial_capital,
            initial_capital: config.initial_capital,
            total_pnl: 0.0,
            total_return_pct: 0.0,
            trades: 0,
            win_rate: 0.0,
            positions: HashMap::new(),
            is_running: false,
            last_bar_time: HashMap::new(),
        }));

        Ok(Self {
            config,
            feed: None,
            executor: std::sync::Mutex::new(executor),
            state,
            bars: HashMap::new(),
            positions: HashMap::new(),
            turtle_state: HashMap::new(),
            last_exit_bar: HashMap::new(),
            completed_trades: Vec::new(),
            gross_profit: 0.0,
            gross_loss: 0.0,
        })
    }

    pub async fn state(&self) -> BotState {
        self.state.read().await.clone()
    }

    /// Start the bot: fetch warmup, connect WebSocket, process bars.
    pub async fn start(&mut self) -> Result<()> {
        tracing::info!(
            "Starting Turtle+Chandelier live bot for {} symbols",
            self.config.symbols.len()
        );
        tracing::info!(
            "Params: EP={}, Chand({},{}), ATR({},{}), HM={}, CAP={}",
            self.config.ep,
            self.config.chand_period, self.config.chand_mult,
            self.config.atr_period, self.config.atr_mult,
            self.config.hold_max, self.config.position_cap
        );

        {
            let mut state = self.state.write().await;
            state.started_at = Some(Utc::now());
            state.is_running = true;
        }

        // Fetch warmup data — need at least EP+1 bars for entry signal
        let warmup_bars = (self.config.ep + self.config.chand_period + 10).max(60);
        for symbol in &self.config.symbols.clone() {
            tracing::info!("Fetching {} warmup bars for {}", warmup_bars, symbol);
            let warmup = fetch_warmup_data(
                symbol,
                &self.config.interval,
                warmup_bars,
            )
            .await
            .context("Failed to fetch warmup data")?;

            tracing::info!("Got {} warmup bars for {}", warmup.len(), symbol);
            self.bars.insert(symbol.clone(), warmup);
        }

        // Connect WebSocket
        let feed = LiveFeed::new(self.config.symbols.clone(), self.config.interval.clone());
        let mut receiver = feed.subscribe();

        feed.start()
            .await
            .context("Failed to start WebSocket feed")?;
        self.feed = Some(feed);

        tracing::info!("Live bot started — processing bars...");

        while let Ok(event) = receiver.recv().await {
            if let Err(e) = self.process_bar(&event).await {
                tracing::error!("Error processing bar: {}", e);
            }
        }

        Ok(())
    }

    pub async fn stop(&mut self) {
        tracing::info!("Stopping live bot");
        if let Some(feed) = &self.feed {
            feed.stop();
        }
        let mut state = self.state.write().await;
        state.is_running = false;
    }

    // =========================================================================
    // Bar processing
    // =========================================================================
    async fn process_bar(&mut self, event: &KlineEvent) -> Result<()> {
        let symbol = &event.symbol;
        let bar = event.kline.to_bar()?;

        // Append bar to history
        if let Some(bars) = self.bars.get_mut(symbol) {
            bars.push(bar.clone());
            if bars.len() > 200 {
                bars.remove(0);
            }
        } else {
            self.bars.insert(symbol.clone(), vec![bar.clone()]);
        }

        // Update state timestamps
        {
            let mut state = self.state.write().await;
            state.last_bar_time.insert(symbol.clone(), bar.time);
        }

        let position = self.positions.get(symbol).copied().unwrap_or_default();
        let current_positions = self.positions.values().filter(|p| !matches!(p, Position::Flat)).count();

        match position {
            Position::Flat => {
                // Check for Turtle breakout entry
                if current_positions < self.config.position_cap {
                    if self.check_turtle_entry(symbol, &bar) {
                        let size = 1.0 / self.config.position_cap as f64;
                        self.open_long(symbol, &bar, size).await?;
                    }
                }
            }
            Position::Long { .. } => {
                // Update trailing state and check dual exit
                if let Some(exit) = self.check_dual_exit(symbol, &bar) {
                    self.close_long(symbol, &bar).await?;
                    // Log slippage summary after each trade cycle
                    if let Ok(exec) = self.executor.lock() {
                        exec.slippage_summary();
                    }
                    return Ok(());
                }
            }
            Position::Short { .. } => {
                // Not used — Turtle is long-only
            }
        }

        Ok(())
    }

    // =========================================================================
    // Turtle breakout entry
    // =========================================================================
    fn check_turtle_entry(&mut self, symbol: &str, bar: &Bar) -> bool {
        let bars = match self.bars.get(symbol) {
            Some(b) => b,
            None => return false,
        };
        let ep = self.config.ep;
        if bars.len() < ep + 1 {
            return false;
        }

        // Freshness filter: skip if exited within cooldown bars
        if let Some(&last_exit) = self.last_exit_bar.get(symbol) {
            let bars_since_exit = bars.len() - last_exit;
            if bars_since_exit < FRESHNESS_COOLDOWN {
                tracing::debug!(
                    "[{}] FRESHNESS SKIP: {} bars since exit (cooldown={})",
                    symbol, bars_since_exit, FRESHNESS_COOLDOWN
                );
                return false;
            }
        }

        // Lookback window: last EP bars (indices len-EP to len-1)
        let ws = bars.len() - ep;
        let max_close = bars[ws..].iter().map(|b| b.close).fold(f64::NEG_INFINITY, f64::max);

        // ATR entry filter: require momentum confirmation beyond ATR noise band
        // Only enter if: close >= max_close + ATR(atr_period) * ATR_ENTRY_MULT
        if ATR_ENTRY_MULT > 0.0 {
            let atr_period = self.config.atr_period;
            let n = bars.len();
            if n >= atr_period + 1 {
                let mut tr_sum = 0.0_f64;
                for i in (n - atr_period)..n {
                    let b = &bars[i];
                    let pc = if i == 0 { b.close } else { bars[i - 1].close };
                    let tr = (b.high - b.low)
                        .max((b.high - pc).abs())
                        .max((b.low - pc).abs());
                    tr_sum += tr;
                }
                let atr = tr_sum / atr_period as f64;
                let threshold = max_close + atr * ATR_ENTRY_MULT;
                if bar.close < threshold {
                    tracing::debug!(
                        "[{}] ATR FILTER SKIP: close {} < threshold {} (ATR={:.2}, EM={})",
                        symbol, bar.close, threshold, atr, ATR_ENTRY_MULT
                    );
                    return false;
                }
            }
        }

        if bar.close >= max_close {
            tracing::info!(
                "[{}] TURTLE ENTRY: close {} >= max_close({}) = {} (ATR_EM={})",
                symbol, bar.close, ep, max_close, ATR_ENTRY_MULT
            );

            // Initialize trailing state
            let mut atr_buf = VecDeque::new();
            let avail = bars.len().min(self.config.chand_period);
            let start = bars.len().saturating_sub(avail);
            for i in 0..avail {
                let idx = start + i;
                if idx >= bars.len() { break; }
                let b = &bars[idx];
                let pc = if i == 0 {
                    b.close
                } else {
                    let prev_idx = start + i - 1;
                    if prev_idx < bars.len() { bars[prev_idx].close } else { b.close }
                };
                let tr = (b.high - b.low).max((b.high - pc).abs()).max((b.low - pc).abs());
                atr_buf.push_back(tr);
            }

            self.turtle_state.insert(symbol.to_string(), TurtleState {
                highest_high: bar.high,
                lowest_low: bar.low,
                bars_held: 0,
                atr_buf,
            });
            return true;
        }
        false
    }

    // =========================================================================
    // Dual exit: Chandelier ATR OR Turtle ATR (whichever fires first)
    // =========================================================================
    fn check_dual_exit(&mut self, symbol: &str, bar: &Bar) -> Option<()> {
        let s = self.turtle_state.get_mut(symbol)?;
        let bars = self.bars.get(symbol)?;

        // Update trailing extremes
        if bar.high > s.highest_high { s.highest_high = bar.high; }
        if bar.low < s.lowest_low { s.lowest_low = bar.low; }
        s.bars_held += 1;

        // Update ATR buffer
        if let Some(prev) = bars.last() {
            let tr = (bar.high - bar.low)
                .max((bar.high - prev.close).abs())
                .max((bar.low - prev.close).abs());
            s.atr_buf.push_back(tr);
        }
        if s.atr_buf.len() > self.config.chand_period {
            s.atr_buf.pop_front();
        }

        // Compute ATR
        if s.atr_buf.len() < self.config.chand_period {
            return None; // Not enough data for ATR
        }
        let atr = s.atr_buf.iter().sum::<f64>() / self.config.chand_period as f64;
        if atr <= 0.0 { return None; }

        // Chandelier trailing stop: highest_high - M * ATR
        let chand_stop = s.highest_high - self.config.chand_mult * atr;

        // Turtle ATR stop: lowest_low - M * ATR
        let turtle_stop = s.lowest_low - self.config.atr_mult * atr;

        // Combined stop: the tighter of the two (higher stop price)
        let stop = chand_stop.max(turtle_stop);

        if bar.low <= stop {
            tracing::info!(
                "[{}] EXIT: low {} <= stop {} (chand={}, turtle={}), held={} bars",
                symbol, bar.low, stop, chand_stop, turtle_stop, s.bars_held
            );
            return Some(());
        }

        // HOLD_MAX timeout
        if s.bars_held >= self.config.hold_max {
            tracing::info!(
                "[{}] EXIT: HOLD_MAX {} reached",
                symbol, self.config.hold_max
            );
            return Some(());
        }

        None
    }

    // =========================================================================
    // Order execution
    // =========================================================================
    async fn open_long(&mut self, symbol: &str, bar: &Bar, size: f64) -> Result<()> {
        let mut exec = self.executor.lock().unwrap();
        exec.place_limit_order(symbol, OrderSide::Buy, size, bar.close).await?;
        drop(exec);

        self.positions.insert(
            symbol.to_string(),
            Position::Long {
                entry_price: bar.close,
                size,
                highest: bar.high,
            },
        );

        tracing::info!(
            "[{}] OPENED LONG @ {} (size {:.3}, equity fraction)",
            symbol, bar.close, size
        );
        self.update_state().await;
        Ok(())
    }

    async fn close_long(&mut self, symbol: &str, bar: &Bar) -> Result<()> {
        let position = self.positions.get(symbol).copied();
        if let Some(Position::Long { entry_price, size, .. }) = position {
            let mut exec = self.executor.lock().unwrap();
            exec.place_limit_order(symbol, OrderSide::Sell, size, bar.close).await?;
            drop(exec);

            let pnl_pct = (bar.close - entry_price) / entry_price * 100.0;
            self.record_trade(symbol, true, entry_price, bar.close, pnl_pct);

            tracing::info!(
                "[{}] CLOSED LONG @ {} (PnL: {:.2}%)",
                symbol, bar.close, pnl_pct
            );

            self.positions.remove(symbol);
            self.turtle_state.remove(symbol);
            // Record exit bar for freshness filter
            if let Some(bars) = self.bars.get(symbol) {
                self.last_exit_bar.insert(symbol.to_string(), bars.len());
            }
        }
        self.update_state().await;
        Ok(())
    }

    fn record_trade(&mut self, _symbol: &str, is_long: bool, entry_price: f64, exit_price: f64, pnl_pct: f64) {
        let equity = self.config.initial_capital;
        let pnl = equity * (pnl_pct / 100.0);

        if pnl > 0.0 {
            self.gross_profit += pnl;
        } else {
            self.gross_loss += pnl.abs();
        }

        self.completed_trades.push(CompletedTrade {
            entry_time: Utc::now(),
            exit_time: Utc::now(),
            entry_price,
            exit_price,
            size: 1.0,
            is_long,
            pnl,
            pnl_pct,
        });
    }

    async fn update_state(&mut self) {
        let mut state = self.state.write().await;
        let fees = self.completed_trades.len() as f64
            * 2.0
            * self.config.fee_pct
            * self.config.initial_capital;
        state.equity = self.config.initial_capital + (self.gross_profit - self.gross_loss) - fees;
        state.total_pnl = state.equity - state.initial_capital;
        state.total_return_pct = (state.equity / state.initial_capital - 1.0) * 100.0;
        state.trades = self.completed_trades.len();

        let wins = self.completed_trades.iter().filter(|t| t.pnl > 0.0).count();
        state.win_rate = if state.trades > 0 {
            wins as f64 / state.trades as f64 * 100.0
        } else {
            0.0
        };

        state.positions.clear();
        for (symbol, pos) in &self.positions {
            let info = match pos {
                Position::Long { entry_price, size, highest } => PositionInfo {
                    symbol: symbol.clone(),
                    side: "LONG".to_string(),
                    size: *size,
                    entry_price: *entry_price,
                    entry_time: Utc::now(),
                    highest: *highest,
                    lowest: 0.0,
                },
                Position::Short { .. } => continue,
                Position::Flat => continue,
            };
            state.positions.insert(symbol.clone(), info);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bot_creation() {
        let config = LiveConfig::default();
        let bot = LiveBot::new(config);
        assert!(bot.is_ok());
    }

    #[test]
    fn test_config_validation() {
        let config = LiveConfig {
            symbols: vec![],
            ..Default::default()
        };
        assert!(LiveBot::new(config).is_err());
    }

    #[tokio::test]
    async fn test_initial_state() {
        let config = LiveConfig::default();
        let bot = LiveBot::new(config).unwrap();
        let state = bot.state().await;

        assert_eq!(state.initial_capital, 10_000.0);
        assert!(!state.is_running);
        assert_eq!(state.trades, 0);
    }

    #[test]
    fn test_turtle_entry_detection() {
        let config = LiveConfig::default();
        let mut bot = LiveBot::new(config).unwrap();

        // Build bar history: 25 bars with closes rising from 90 to 114
        let bars: Vec<Bar> = (0..25)
            .map(|i| Bar::new(Utc::now(), 90.0 + i as f64, 91.0 + i as f64, 89.0 + i as f64, 90.0 + i as f64, 1000.0 + i as f64))
            .collect();
        // max_close of last 21 bars = 114.0
        bot.bars.insert("BTCUSDT".to_string(), bars.clone());

        // Bar closing at 105 — no breakout (105 < max_close = 114)
        let bar_no_break = Bar::new(Utc::now(), 105.0, 106.0, 104.0, 105.0, 1000.0);
        assert!(!bot.check_turtle_entry("BTCUSDT", &bar_no_break));

        // Bar closing at 115 — breakout (115 >= max_close = 114)
        let bar_break = Bar::new(Utc::now(), 115.0, 116.0, 114.0, 115.0, 2000.0);
        assert!(bot.check_turtle_entry("BTCUSDT", &bar_break));
        assert!(bot.turtle_state.contains_key("BTCUSDT"));
    }

    #[test]
    fn test_position_cap_enforced() {
        let mut config = LiveConfig::default();
        config.position_cap = 2;
        let mut bot = LiveBot::new(config).unwrap();

        // Simulate 2 existing positions
        bot.positions.insert("BTCUSDT".to_string(), Position::Long {
            entry_price: 50000.0, size: 0.5, highest: 51000.0,
        });
        bot.positions.insert("ETHUSDT".to_string(), Position::Long {
            entry_price: 3000.0, size: 0.5, highest: 3100.0,
        });

        // Build bars for SOLUSDT with breakout
        let bars: Vec<Bar> = (0..25)
            .map(|i| Bar::new(Utc::now(), 100.0, 101.0, 99.0, 100.0, 1000.0 + i as f64))
            .collect();
        bot.bars.insert("SOLUSDT".to_string(), bars);

        // Breakout bar
        let bar = Bar::new(Utc::now(), 101.0, 102.0, 100.0, 101.0, 2000.0);

        // Should detect entry signal but cap blocks it
        let current_positions = bot.positions.values()
            .filter(|p| !matches!(p, Position::Flat)).count();
        assert_eq!(current_positions, 2);
        assert!(current_positions >= bot.config.position_cap);
    }
}
