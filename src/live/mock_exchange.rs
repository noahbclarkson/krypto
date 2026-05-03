//! Mock exchange for backtesting live bot execution logic.
//!
//! Simulates Binance USDT-M futures fills against seeded historical klines.
//! Models: market/limit/stop orders, partial fills, slippage, position tracking.
//!
//! # Design
//!
//! The exchange owns historical bars. Each call to [`MockExchange::advance_bar`]
//! advances the simulation by one bar and processes all pending orders for fills.
//! Market orders placed via [`MockExchange::place_market_order`] fill immediately
//! at the current (most-recently-processed) bar's close price.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// =============================================================================
// Public Types
// =============================================================================

/// A historical OHLCV bar used to seed the simulation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bar {
    pub open_time: i64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
    pub close_time: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MockOrderType { Limit, Market, StopLoss }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MockSide { Buy, Sell }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MockOrderStatus { New, Filled, Cancelled }

#[derive(Debug, Clone)]
pub struct MockOrder {
    pub client_order_id: String,
    pub symbol: String,
    pub side: MockSide,
    pub order_type: MockOrderType,
    pub quantity: f64,
    pub price: Option<f64>,
    pub stop_price: Option<f64>,
    pub status: MockOrderStatus,
    pub avg_fill_price: Option<f64>,
    pub bars_remaining: usize,
    pub created_bar: usize,
}

#[derive(Debug, Clone)]
pub struct MockPosition {
    pub symbol: String,
    pub side: MockSide,
    pub quantity: f64,
    pub entry_price: f64,
    pub open_bar: usize,
}

#[derive(Debug, Clone)]
pub struct MockFill {
    pub client_order_id: String,
    pub symbol: String,
    pub side: String,
    pub order_type: String,
    pub price: f64,
    pub quantity: f64,
    pub fee_paid: f64,
    pub slippage_bp: f64,
    pub bar_idx: usize,
    pub timestamp: DateTime<Utc>,
}

// =============================================================================
// Config
// =============================================================================

/// Simulation configuration.
#[derive(Debug, Clone)]
pub struct MockExchangeConfig {
    /// Fee rate per side (e.g., 0.0004 = 4 bps).
    pub fee_pct: f64,
    /// Probability a limit order fills when price condition is met.
    pub limit_fill_prob: f64,
    /// Slippage in bp for market orders.
    pub market_slippage_bp: f64,
    /// Probability a market order fills immediately.
    pub market_fill_prob: f64,
    /// Max bars before GTC order expires.
    pub max_gfc_bars: usize,
}

impl Default for MockExchangeConfig {
    fn default() -> Self {
        Self {
            fee_pct: 0.0004,
            limit_fill_prob: 0.80,
            market_slippage_bp: 5.0,
            market_fill_prob: 0.95,
            max_gfc_bars: 500,
        }
    }
}

impl MockExchangeConfig {
    /// Maker-favored: tighter fills, lower fees.
    pub fn maker() -> Self {
        Self {
            fee_pct: 0.0002,
            limit_fill_prob: 0.90,
            market_slippage_bp: 2.0,
            market_fill_prob: 0.80,
            max_gfc_bars: 500,
        }
    }

    /// Blended realistic: ~70% maker / 30% taker for Turtle breakout entries.
    pub fn realistic() -> Self {
        Self {
            fee_pct: 0.00028,
            limit_fill_prob: 0.80,
            market_slippage_bp: 3.0,
            market_fill_prob: 0.85,
            max_gfc_bars: 500,
        }
    }
}

// =============================================================================
// MockExchange
// =============================================================================

pub struct MockExchange {
    cfg: MockExchangeConfig,
    bars: Vec<Bar>,
    bar_idx: usize,
    positions: HashMap<String, MockPosition>,
    orders: HashMap<String, MockOrder>,
    fills: Vec<MockFill>,
    next_oid: u64,
    rng_state: u64,
}

impl MockExchange {
    /// Seed with historical bars.
    pub fn new(bars: Vec<Bar>, cfg: MockExchangeConfig) -> Self {
        Self {
            cfg,
            bars,
            bar_idx: 0,
            positions: HashMap::new(),
            orders: HashMap::new(),
            fills: Vec::new(),
            next_oid: 1,
            rng_state: 0x_C0FFEE_BADC0DE_u64,
        }
    }

    pub fn current_bar_idx(&self) -> usize { self.bar_idx }
    pub fn total_bars(&self) -> usize { self.bars.len() }
    pub fn positions(&self) -> &HashMap<String, MockPosition> { &self.positions }
    pub fn fills(&self) -> &[MockFill] { &self.fills }
    pub fn orders(&self) -> &HashMap<String, MockOrder> { &self.orders }

    pub fn has_position(&self, symbol: &str) -> bool {
        self.positions.contains_key(symbol)
    }

    pub fn get_position(&self, symbol: &str) -> Option<&MockPosition> {
        self.positions.get(symbol)
    }

    /// Unrealized PnL at current prices.
    pub fn unrealized_pnl(&self, prices: &HashMap<String, f64>) -> f64 {
        self.positions.iter().map(|(sym, pos)| {
            let cur = *prices.get(sym).unwrap_or(&pos.entry_price);
            match pos.side {
                MockSide::Buy => (cur - pos.entry_price) * pos.quantity,
                MockSide::Sell => (pos.entry_price - cur) * pos.quantity,
            }
        }).sum()
    }

    /// Realized PnL from closed trades.
    pub fn realized_pnl(&self) -> f64 {
        let mut net = 0.0;
        let mut long_avg: HashMap<String, (f64, f64)> = HashMap::new(); // (avg_price, qty)

        for fill in &self.fills {
            let fee = fill.fee_paid;
            match fill.side.as_str() {
                "BUY" => {
                    let entry = long_avg.entry(fill.symbol.clone()).or_insert((0.0, 0.0));
                    let total = entry.1 + fill.quantity;
                    if total > 0.0 {
                        entry.0 = (entry.0 * entry.1 + fill.price * fill.quantity) / total;
                    }
                    entry.1 = total;
                    net -= fill.price * fill.quantity;
                    net -= fee;
                }
                "SELL" => {
                    if let Some((avg_price, qty)) = long_avg.get(&fill.symbol) {
                        if *qty > 0.0 {
                            let close_qty = fill.quantity.min(*qty);
                            net += fill.price * close_qty;
                            net -= avg_price * close_qty;
                            net -= fee;
                        }
                    } else {
                        net += fill.price * fill.quantity;
                        net -= fee;
                    }
                }
                _ => {}
            }
        }
        net
    }

    // -------------------------------------------------------------------------
    // Order Placement
    // -------------------------------------------------------------------------

    fn next_cid(&mut self, symbol: &str) -> String {
        let id = self.next_oid;
        self.next_oid += 1;
        format!("mock_{}_{}", symbol, id)
    }

    /// Place a limit order. Returns client order ID.
    pub fn place_limit_order(&mut self, symbol: &str, side: MockSide, quantity: f64, price: f64) -> String {
        let cid = self.next_cid(symbol);
        let order = MockOrder {
            client_order_id: cid.clone(),
            symbol: symbol.to_string(),
            side,
            order_type: MockOrderType::Limit,
            quantity,
            price: Some(price),
            stop_price: None,
            status: MockOrderStatus::New,
            avg_fill_price: None,
            bars_remaining: self.cfg.max_gfc_bars,
            created_bar: self.bar_idx,
        };
        self.orders.insert(cid.clone(), order);
        cid
    }

    /// Place a market order. Fills immediately at current bar close.
    pub fn place_market_order(&mut self, symbol: &str, side: MockSide, quantity: f64) -> String {
        let cid = self.next_cid(symbol);
        let order = MockOrder {
            client_order_id: cid.clone(),
            symbol: symbol.to_string(),
            side,
            order_type: MockOrderType::Market,
            quantity,
            price: None,
            stop_price: None,
            status: MockOrderStatus::New,
            avg_fill_price: None,
            bars_remaining: 0,
            created_bar: self.bar_idx,
        };
        self.orders.insert(cid.clone(), order);

        // Immediate fill at most-recently-processed bar
        let bar_opt = self.bars.get(self.bar_idx.saturating_sub(1)).cloned()
            .or_else(|| self.bars.first().cloned());
        if let Some(bar) = bar_opt {
            let o = self.orders.get(&cid).cloned().unwrap();
            self.do_fill(&o, &bar);
        }
        cid
    }

    /// Place a stop-loss order.
    pub fn place_stop_loss(&mut self, symbol: &str, side: MockSide, quantity: f64, stop_price: f64) -> String {
        let cid = self.next_cid(symbol);
        let order = MockOrder {
            client_order_id: cid.clone(),
            symbol: symbol.to_string(),
            side,
            order_type: MockOrderType::StopLoss,
            quantity,
            price: None,
            stop_price: Some(stop_price),
            status: MockOrderStatus::New,
            avg_fill_price: None,
            bars_remaining: self.cfg.max_gfc_bars,
            created_bar: self.bar_idx,
        };
        self.orders.insert(cid.clone(), order);
        cid
    }

    /// Cancel a pending order.
    pub fn cancel_order(&mut self, client_order_id: &str) -> bool {
        if let Some(o) = self.orders.get_mut(client_order_id) {
            if o.status == MockOrderStatus::New {
                o.status = MockOrderStatus::Cancelled;
                return true;
            }
        }
        false
    }

    // -------------------------------------------------------------------------
    // Simulation Advance
    // -------------------------------------------------------------------------

    /// Advance one bar. Returns symbols that had fills this bar.
    pub fn advance_bar(&mut self) -> Vec<String> {
        if self.bar_idx >= self.bars.len() {
            return Vec::new();
        }
        let bar = self.bars[self.bar_idx].clone();
        self.bar_idx += 1;

        // Tick down GTC timers
        for o in self.orders.values_mut() {
            if o.bars_remaining > 0 { o.bars_remaining -= 1; }
        }

        // Collect fills to process
        let pending: Vec<MockOrder> = self.orders
            .values()
            .filter(|o| o.status == MockOrderStatus::New)
            .cloned()
            .collect();

        let mut filled_syms = Vec::new();
        for order in pending {
            if self.try_fill(&order, &bar) {
                filled_syms.push(order.symbol.clone());
            }
        }
        filled_syms
    }

    pub fn advance_n(&mut self, n: usize) {
        for _ in 0..n { self.advance_bar(); }
    }

    pub fn run_to_end(&mut self) {
        while self.bar_idx < self.bars.len() { self.advance_bar(); }
    }

    // -------------------------------------------------------------------------
    // Fill Logic
    // -------------------------------------------------------------------------

    fn try_fill(&mut self, order: &MockOrder, bar: &Bar) -> bool {
        // GTC expiry for limit orders
        if order.bars_remaining == 0 && order.order_type == MockOrderType::Limit {
            if let Some(o) = self.orders.get_mut(&order.client_order_id) {
                o.status = MockOrderStatus::Cancelled;
            }
            return false;
        }

        let triggered = match order.order_type {
            MockOrderType::Market => self.rng() < self.cfg.market_fill_prob,
            MockOrderType::Limit => {
                self.limit_triggered(order, bar) && self.rng() < self.cfg.limit_fill_prob
            }
            MockOrderType::StopLoss => {
                self.stop_triggered(order, bar) && self.rng() < self.cfg.limit_fill_prob
            }
        };

        if triggered {
            self.do_fill(order, bar);
            true
        } else {
            false
        }
    }

    fn limit_triggered(&self, order: &MockOrder, bar: &Bar) -> bool {
        let price = match order.price { Some(p) => p, None => return false };
        match order.side {
            MockSide::Buy => bar.low <= price,
            MockSide::Sell => bar.high >= price,
        }
    }

    fn stop_triggered(&self, order: &MockOrder, bar: &Bar) -> bool {
        let sp = match order.stop_price { Some(p) => p, None => return false };
        match order.side {
            MockSide::Buy => bar.low <= sp,
            MockSide::Sell => bar.high >= sp,
        }
    }

    fn do_fill(&mut self, order: &MockOrder, bar: &Bar) {
        let fill_price = match order.order_type {
            MockOrderType::Market => {
                let slip = self.cfg.market_slippage_bp / 10_000.0;
                match order.side {
                    MockSide::Buy => bar.close * (1.0 + slip),
                    MockSide::Sell => bar.close * (1.0 - slip),
                }
            }
            MockOrderType::StopLoss => order.stop_price.unwrap_or(bar.close),
            _ => order.price.unwrap_or(bar.close),
        };

        let qty = order.quantity;
        let fee = fill_price * qty * self.cfg.fee_pct;

        let fill = MockFill {
            client_order_id: order.client_order_id.clone(),
            symbol: order.symbol.clone(),
            side: if order.side == MockSide::Buy { "BUY".into() } else { "SELL".into() },
            order_type: format!("{:?}", order.order_type),
            price: fill_price,
            quantity: qty,
            fee_paid: fee,
            slippage_bp: self.cfg.market_slippage_bp,
            bar_idx: self.bar_idx,
            timestamp: Utc::now(),
        };
        self.fills.push(fill);

        // Update position
        self.update_position(order, fill_price, qty);

        // Mark order filled
        if let Some(o) = self.orders.get_mut(&order.client_order_id) {
            o.status = MockOrderStatus::Filled;
            o.avg_fill_price = Some(fill_price);
        }
    }

    fn update_position(&mut self, order: &MockOrder, price: f64, qty: f64) {
        let sym = &order.symbol;
        use std::collections::hash_map::Entry;

        match order.side {
            MockSide::Buy => {
                match self.positions.entry(sym.clone()) {
                    Entry::Occupied(mut e) => {
                        let p = e.get_mut();
                        let total = p.quantity + qty;
                        p.entry_price = (p.entry_price * p.quantity + price * qty) / total;
                        p.quantity = total;
                    }
                    Entry::Vacant(e) => {
                        e.insert(MockPosition {
                            symbol: sym.clone(),
                            side: MockSide::Buy,
                            quantity: qty,
                            entry_price: price,
                            open_bar: self.bar_idx,
                        });
                    }
                }
            }
            MockSide::Sell => {
                // Closing existing long?
                if let Some(pos) = self.positions.get(sym) {
                    if pos.side == MockSide::Buy {
                        // Partial or full close of long
                        if qty >= pos.quantity {
                            let excess = qty - pos.quantity;
                            self.positions.remove(sym);
                            if excess > 1e-8 {
                                // Convert to short for the excess
                                self.positions.insert(sym.clone(), MockPosition {
                                    symbol: sym.clone(),
                                    side: MockSide::Sell,
                                    quantity: excess,
                                    entry_price: price,
                                    open_bar: self.bar_idx,
                                });
                            }
                        } else {
                            // Reduce long position
                            if let Some(p) = self.positions.get_mut(sym) {
                                p.quantity -= qty;
                            }
                        }
                        return;
                    }
                }
                // Open or add to short
                match self.positions.entry(sym.clone()) {
                    Entry::Occupied(mut e) => {
                        let p = e.get_mut();
                        let total = p.quantity + qty;
                        p.entry_price = (p.entry_price * p.quantity + price * qty) / total;
                        p.quantity = total;
                    }
                    Entry::Vacant(e) => {
                        e.insert(MockPosition {
                            symbol: sym.clone(),
                            side: MockSide::Sell,
                            quantity: qty,
                            entry_price: price,
                            open_bar: self.bar_idx,
                        });
                    }
                }
            }
        }
    }

    fn rng(&mut self) -> f64 {
        let mut x = self.rng_state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng_state = x;
        // Top 53 bits as [0, 1)
        const MASK: u64 = (1u64 << 53) - 1;
        (x & MASK) as f64 / (1u64 << 53) as f64
    }

    // -------------------------------------------------------------------------
    // Reporting
    // -------------------------------------------------------------------------

    pub fn summary(&self) {
        println!("\n=== MockExchange ===");
        println!("Bars: {}/{} | Fills: {} | Positions: {}",
            self.bar_idx.min(self.bars.len()), self.bars.len(),
            self.fills.len(), self.positions.len());
        let total_fees: f64 = self.fills.iter().map(|f| f.fee_paid).sum();
        println!("Total fees: ${:.4} | Realized PnL: ${:.4}", total_fees, self.realized_pnl());
        for (sym, pos) in &self.positions {
            println!("  {} {} {} @ {}",
                if pos.side == MockSide::Buy { "LONG" } else { "SHORT" },
                sym, pos.quantity, pos.entry_price);
        }
        for f in self.fills.iter().rev().take(10) {
            println!("  {:<6} {:<10} {:>8.4} @ {:>10.4} [bar {}]",
                f.side, f.symbol, f.quantity, f.price, f.bar_idx);
        }
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn bar(i: usize, o: f64, h: f64, l: f64, c: f64) -> Bar {
        Bar { open_time: i as i64, open: o, high: h, low: l, close: c, volume: 1000.0, close_time: i as i64 + 1 }
    }

    fn trending_up(n: usize) -> Vec<Bar> {
        (0..n).map(|i| bar(i, 100.0 + i as f64, 101.0 + i as f64, 99.5 + i as f64, 100.5 + i as f64)).collect()
    }

    fn cfg100() -> MockExchangeConfig {
        MockExchangeConfig {
            fee_pct: 0.0,
            limit_fill_prob: 1.0,
            market_slippage_bp: 0.0,
            market_fill_prob: 1.0,
            max_gfc_bars: 500,
        }
    }

    #[test]
    fn test_market_fill_immediate() {
        let bars = trending_up(5);
        let mut ex = MockExchange::new(bars, cfg100());
        ex.place_market_order("BTC", MockSide::Buy, 1.0);
        assert_eq!(ex.fills().len(), 1);
        let p = ex.get_position("BTC").expect("position must exist");
        assert_eq!(p.quantity, 1.0);
        assert_eq!(p.side, MockSide::Buy);
    }

    #[test]
    fn test_limit_buy_on_drop() {
        // Bars 0-2: low=98 (below limit 99.5) → fills
        // Bars 3+: low=103+ (above limit) → no fill
        let bars: Vec<Bar> = (0..8).map(|i| {
            if i < 3 { bar(i, 99.0, 100.0, 98.0, 98.5) }
            else { bar(i, 103.0 + i as f64, 104.0 + i as f64, 102.0 + i as f64, 103.5 + i as f64) }
        }).collect();

        let mut ex = MockExchange::new(bars, cfg100());
        let oid = ex.place_limit_order("BTC", MockSide::Buy, 1.0, 99.5);
        assert!(ex.fills().is_empty()); // no bar advanced yet

        ex.advance_bar(); // bar 0: low=98 <= 99.5 → fill
        assert_eq!(ex.fills().len(), 1, "limit should fill at bar 0");
        assert_eq!(ex.orders().get(&oid).unwrap().status, MockOrderStatus::Filled);
        assert!(ex.has_position("BTC"));
    }

    #[test]
    fn test_limit_sell_on_rise() {
        // Bars 0-2: high=102 (above limit 101) → fills
        let bars: Vec<Bar> = (0..6).map(|i| {
            if i < 3 { bar(i, 100.0, 102.0, 99.0, 101.5) }
            else { bar(i, 95.0, 97.0, 94.0, 95.5) }
        }).collect();

        let mut ex = MockExchange::new(bars, cfg100());
        ex.place_market_order("BTC", MockSide::Buy, 1.0); // entry first
        let oid = ex.place_limit_order("BTC", MockSide::Sell, 1.0, 101.0);

        ex.advance_bar(); // bar 0: high=102 >= 101 → fill limit
        assert_eq!(ex.fills().len(), 2); // market + limit
        assert_eq!(ex.orders().get(&oid).unwrap().status, MockOrderStatus::Filled);
        assert!(!ex.has_position("BTC"), "long should be closed by sell limit");
    }

    #[test]
    fn test_stop_loss_triggers() {
        // Price rises from 100 to 115
        let bars: Vec<Bar> = (0..20).map(|i| {
            let p = 100.0 + i as f64;
            bar(i, p - 0.5, p + 1.0, p - 1.0, p)
        }).collect();

        let mut ex = MockExchange::new(bars, cfg100());
        ex.place_market_order("BTC", MockSide::Sell, 1.0); // short entry
        let sl_oid = ex.place_stop_loss("BTC", MockSide::Buy, 1.0, 108.0); // stop at 108

        // Advance until price >= 108 (bar 8)
        for _ in 0..15 { ex.advance_bar(); }

        let sl = ex.orders().get(&sl_oid).unwrap();
        assert_eq!(sl.status, MockOrderStatus::Filled, "stop-loss should trigger at 108");
    }

    #[test]
    fn test_partial_close_long() {
        let bars = trending_up(5);
        let mut ex = MockExchange::new(bars, cfg100());

        ex.place_market_order("BTC", MockSide::Buy, 1.0);   // long 1.0
        assert!((ex.get_position("BTC").unwrap().quantity - 1.0).abs() < 1e-8);

        ex.place_market_order("BTC", MockSide::Sell, 0.3);  // close 0.3
        let p = ex.get_position("BTC").unwrap();
        assert!((p.quantity - 0.7).abs() < 1e-8, "expected 0.7, got {}", p.quantity);

        ex.place_market_order("BTC", MockSide::Sell, 0.8);  // close 0.7 + 0.1 short
        let p = ex.get_position("BTC").unwrap();
        assert!((p.quantity - 0.1).abs() < 1e-8, "expected 0.1 short, got {}", p.quantity);
        assert_eq!(p.side, MockSide::Sell);
    }

    #[test]
    fn test_gtc_expiry() {
        let bars = trending_up(3);
        let mut cfg = cfg100();
        cfg.max_gfc_bars = 2;
        let mut ex = MockExchange::new(bars, cfg);

        let oid = ex.place_limit_order("BTC", MockSide::Buy, 1.0, 50.0); // never triggers
        ex.advance_bar(); // bars_remaining: 2→1
        ex.advance_bar(); // bars_remaining: 1→0, expires
        assert_eq!(ex.orders().get(&oid).unwrap().status, MockOrderStatus::Cancelled);
    }

    #[test]
    fn test_cancel_order() {
        let bars = trending_up(3);
        let mut ex = MockExchange::new(bars, cfg100());

        // Cancel a New order → succeeds
        let oid_new = ex.place_limit_order("BTC", MockSide::Buy, 1.0, 99.0);
        assert!(ex.cancel_order(&oid_new));
        assert_eq!(ex.orders().get(&oid_new).unwrap().status, MockOrderStatus::Cancelled);

        // Cancel a Filled order → fails (status != New)
        ex.place_market_order("BTC", MockSide::Buy, 1.0); // fills immediately
        let filled_oid = ex.orders().values().find(|o| o.status == MockOrderStatus::Filled).map(|o| o.client_order_id.clone()).unwrap();
        assert!(!ex.cancel_order(&filled_oid), "Cannot cancel a filled order");

        // Cancel a Cancelled order → fails (status != New)
        assert!(!ex.cancel_order(&oid_new), "Cannot cancel an already-cancelled order");
    }

    #[test]
    fn test_no_fill_without_advance() {
        // Place limit before any bars processed — should not fill
        let bars = trending_up(5);
        let mut ex = MockExchange::new(bars, cfg100());
        ex.place_limit_order("BTC", MockSide::Buy, 1.0, 50.0); // never triggers
        assert!(ex.fills().is_empty());
        assert_eq!(ex.orders().get("mock_BTC_1").unwrap().status, MockOrderStatus::New);
    }
}
