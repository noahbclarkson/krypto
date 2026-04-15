//! Daily Mean Reversion Walk-Forward Validation
//!
//! Tests daily z-score MR on crypto as a standalone strategy.
//! 252-bar train / 63-bar test windows, step 21 bars.
//!
//! ```bash
//! cargo run --example daily_mean_reversion_walkforward --profile sweep
//! ```

use anyhow::Result;
use chrono::{TimeZone, Utc};
use krypto::data::loader::DataLoader;
use krypto::paper::{Bar, PaperBot, Strategy};

const Z_LOOKBACK: usize = 20;
const Z_ENTRY: f64 = 1.5;
const Z_EXIT: f64 = 0.3;
const MAX_HOLD: usize = 10;
const STOP_PCT: f64 = 0.03;
const FEE_PCT: f64 = 0.0004;
const MIN_TRADES: usize = 2;
const TRAIN: usize = 252;
const TEST: usize = 63;
const STEP: usize = 21;

struct DailyMeanReversion {
    ma_buf: std::collections::VecDeque<f64>,
    entry_px: f64,
    bars_held: usize,
    is_long: bool,
    in_pos: bool,
}

impl DailyMeanReversion {
    fn new() -> Self {
        Self {
            ma_buf: std::collections::VecDeque::with_capacity(Z_LOOKBACK + 1),
            entry_px: 0.0,
            bars_held: 0,
            is_long: true,
            in_pos: false,
        }
    }
    fn z(&self, price: f64) -> f64 {
        if self.ma_buf.len() < 2 { return 0.0; }
        let n = self.ma_buf.len() as f64;
        let mean = self.ma_buf.iter().sum::<f64>() / n;
        let sd = ((self.ma_buf.iter().map(|x| (x - mean).powi(2)).sum::<f64>()) / n).sqrt();
        if sd < 1e-10 { return 0.0; }
        (price - mean) / sd
    }
}

impl Default for DailyMeanReversion { fn default() -> Self { Self::new() } }

impl Strategy for DailyMeanReversion {
    fn name(&self) -> &str { "DailyMR" }
    fn on_bar(&mut self, bar: &Bar, pos: f64, _h: &[Bar]) -> Option<krypto::paper::Trade> {
        self.ma_buf.push_back(bar.close);
        if self.ma_buf.len() > Z_LOOKBACK { self.ma_buf.pop_front(); }
        if self.ma_buf.len() < Z_LOOKBACK { return None; }
        let z = self.z(bar.close);

        if pos != 0.0 {
            self.bars_held += 1;
            let pnl = if self.is_long {
                (bar.close - self.entry_px) / self.entry_px
            } else {
                (self.entry_px - bar.close) / self.entry_px
            };
            if pnl <= -STOP_PCT || self.bars_held >= MAX_HOLD {
                self.in_pos = false;
                return Some(krypto::paper::Trade::Close);
            }
            let exit = if self.is_long { z >= Z_EXIT } else { z <= -Z_EXIT };
            if exit && self.bars_held >= 2 {
                self.in_pos = false;
                return Some(krypto::paper::Trade::Close);
            }
            return None;
        }

        self.bars_held = 0;
        if z <= -Z_ENTRY {
            self.entry_px = bar.close;
            self.is_long = true;
            self.in_pos = true;
            return Some(krypto::paper::Trade::Long { size: 1.0 });
        }
        if z >= Z_ENTRY {
            self.entry_px = bar.close;
            self.is_long = false;
            self.in_pos = true;
            return Some(krypto::paper::Trade::Short { size: 1.0 });
        }
        None
    }
}

struct Wr {
    n: usize, wr: f64, sh: f64, dd: f64, pf: f64,
    ret: f64, gw: f64, gl: f64, wins: f64, eq: f64,
}

fn run_sym(sym: &str, candles: u32) -> Result<Vec<Wr>> {
    let loader = DataLoader::new(None, None);
    let rt = tokio::runtime::Runtime::new()?;
    let df = rt.block_on(async { loader.fetch_data(sym, "1d", candles).await })?;

    let tc = df.column("time")?.datetime()?;
    let cc = df.column("close")?.f64()?;

    let mut bars = Vec::with_capacity(df.height());
    for i in 0..df.height() {
        let ms = tc.get(i).unwrap_or(0);
        let dt = Utc.timestamp_opt(ms / 1000, 0).unwrap();
        bars.push(Bar { time: dt, open: 0.0, high: 0.0, low: 0.0, close: cc.get(i).unwrap_or(0.0), volume: 0.0 });
    }

    let warm = Z_LOOKBACK + MAX_HOLD + TEST;
    if bars.len() < warm { return Ok(Vec::new()); }

    let mut results = Vec::new();
    let mut t = TRAIN;
    while t + TEST <= bars.len() {
        let test = &bars[t..t + TEST];
        let mut bot = PaperBot::new(Box::new(DailyMeanReversion::new()), 10_000.0).with_fee(FEE_PCT);
        for b in test { bot.on_bar(b); }
        let ts = bot.trades();
        let n = ts.len();
        if n >= MIN_TRADES {
            let wins = ts.iter().filter(|x| x.pnl_pct > 0.0).count() as f64;
            let wr = wins / n as f64 * 100.0;
            let ret = ts.iter().map(|x| x.pnl_pct).sum::<f64>() / n as f64;
            let gw: f64 = ts.iter().filter(|x| x.pnl_pct > 0.0).map(|x| x.pnl_pct).sum();
            let gl: f64 = ts.iter().filter(|x| x.pnl_pct <= 0.0).map(|x| x.pnl_pct.abs()).sum();
            let pf = if gl > 1e-10 { gw / gl } else { f64::INFINITY };
            let dr: Vec<f64> = ts.iter().map(|x| x.pnl_pct / 5.0).collect();
            let mn = if dr.is_empty() { 0.0 } else { dr.iter().sum::<f64>() / dr.len() as f64 };
            let sd = if dr.len() < 2 { 0.0 } else { (dr.iter().map(|x| (x - mn).powi(2)).sum::<f64>() / dr.len() as f64).sqrt() };
            let sh = if sd > 1e-10 { mn * 365.0_f64.sqrt() / sd } else { 0.0 };
            let sm = bot.summary();
            results.push(Wr { n, wr, sh, dd: sm.max_drawdown_pct, pf, ret, gw, gl, wins, eq: bot.equity() / 10_000.0 });
        }
        t += STEP;
    }
    Ok(results)
}

fn main() -> Result<()> {
    use colored::*;
    if std::env::var("RUST_LOG").unwrap_or_default().is_empty() { std::env::set_var("RUST_LOG", "warn"); }
    tracing_subscriber::fmt::init();

    println!();
    println!("{}", "=".repeat(70).bold().cyan());
    println!("  Daily Mean Reversion Walk-Forward");
    println!("  Z=±{:.1}, LB={}, Exit=±{:.1}, MaxHold={}", Z_ENTRY, Z_LOOKBACK, Z_EXIT, MAX_HOLD);
    println!("{}", "=".repeat(70).bold().cyan());
    println!();

    let syms = [("BTCUSDT", 1500u32), ("ETHUSDT", 1500), ("SOLUSDT", 1200),
                ("XRPUSDT", 1200), ("DOGEUSDT", 1000), ("ADAUSDT", 1000)];

    let mut all: Vec<(String, Wr)> = Vec::new();
    for (sym, n) in syms {
        print!("  {:12} ", sym.yellow());
        let t0 = std::time::Instant::now();
        match run_sym(sym, n) {
            Ok(rs) => {
                let took = t0.elapsed();
                let tw: usize = rs.iter().map(|r| r.n).sum();
                let pass = rs.iter().filter(|r| r.sh > 0.0).count();
                let avg_sh = if rs.is_empty() { 0.0 } else { rs.iter().map(|r| r.sh).sum::<f64>() / rs.len() as f64 };
                let avg_wr = if rs.is_empty() { 0.0 } else { rs.iter().map(|r| r.wr).sum::<f64>() / rs.len() as f64 };
                let pass_rate = if rs.is_empty() { 0.0 } else { pass as f64 / rs.len() as f64 * 100.0 };
                let icon = if pass == rs.len() && !rs.is_empty() { "✓" } else if pass > 0 { "~" } else { "✗" };
                println!("{} {} wins {} trades | pass {}/{} {:.0}% | sh {:.2} | wr {:.0}% | {:.1}s",
                    icon, rs.len(), tw, pass, rs.len(), pass_rate, avg_sh, avg_wr, took.as_secs_f32());
                for r in rs { all.push((sym.to_string(), r)); }
            }
            Err(e) => println!("FAIL: {}", e),
        }
    }

    if all.is_empty() { println!("  No results."); return Ok(()); }

    let nw = all.len();
    let tt: usize = all.iter().map(|(_, r)| r.n).sum();
    let pass = all.iter().filter(|(_, r)| r.sh > 0.0).count();
    let wr = all.iter().map(|(_, r)| r.wr).sum::<f64>() / nw as f64;
    let sh = all.iter().map(|(_, r)| r.sh).sum::<f64>() / nw as f64;
    let dd = all.iter().map(|(_, r)| r.dd).fold(0.0f64, |a, b| a.max(b));
    let gw_all: f64 = all.iter().map(|(_, r)| r.gw).sum();
    let gl_all: f64 = all.iter().map(|(_, r)| r.gl).sum();
    let pf_all = if gl_all > 1e-10 { gw_all / gl_all } else { f64::INFINITY };
    let ret_all = all.iter().map(|(_, r)| r.ret).sum::<f64>() / nw as f64;
    let wins_all: f64 = all.iter().map(|(_, r)| r.wins).sum();
    let avg_eq = all.iter().map(|(_, r)| r.eq).sum::<f64>() / nw as f64;

    println!();
    println!("  {}", "-- Aggregate --".cyan());
    println!();
    println!("  {:<28} {:>10}", "Metric", "Value");
    println!("  {}", "-".repeat(42));
    println!("  {:<28} {:>10}", "Windows tested", format!("{}", nw));
    println!("  {:<28} {:>10}", "Total trades", format!("{}", tt));
    println!("  {:<28} {:>9.0}%", "Pass rate (sh>0)", pass as f64 / nw as f64 * 100.0);
    println!("  {:<28} {:>10.2}", "Avg Sharpe", sh);
    println!("  {:<28} {:>9.0}%", "Avg Win Rate", wr);
    println!("  {:<28} {:>10.2}", "Profit Factor", pf_all);
    println!("  {:<28} {:>9.0}%", "Worst Max DD", dd);
    println!("  {:<28} {:>10.3}%", "Avg Return/Trade", ret_all);
    println!("  {:<28} {:>10.2}%", "Avg Win", gw_all / wins_all.max(1.0));
    println!("  {:<28} {:>10.2}%", "Avg Loss", -(gl_all / (tt as f64 - wins_all).max(1.0)));
    println!("  {:<28} {:>10.4}x", "Avg Final Equity", avg_eq);
    println!();

    let mut by_s: std::collections::HashMap<String, Vec<&Wr>> = std::collections::HashMap::new();
    for (s, r) in &all { by_s.entry(s.clone()).or_default().push(r); }

    println!("  {}", "-- Per-Symbol --".cyan());
    println!();
    println!("  {:<10} {:>7} {:>8} {:>8} {:>8} {:>7}", "Sym", "WinRate", "Sharpe", "MaxDD%", "AvgRet%", "PF");
    println!("  {}", "-".repeat(52));
    for (s, rs) in by_s {
        let n = rs.len();
        let aws = rs.iter().map(|r| r.sh).sum::<f64>() / n as f64;
        let wrs = rs.iter().map(|r| r.wr).sum::<f64>() / n as f64;
        let dds = rs.iter().map(|r| r.dd).fold(0.0f64, |a, b| a.max(b));
        let rts = rs.iter().map(|r| r.ret).sum::<f64>() / n as f64;
        let gws: f64 = rs.iter().map(|r| r.gw).sum();
        let gls: f64 = rs.iter().map(|r| r.gl).sum();
        let pf_s = if gls > 1e-10 { gws / gls } else { f64::INFINITY };
        let pf_str = if pf_s.is_infinite() { "∞".to_string() } else { format!("{:.2}", pf_s) };
        println!("  {:<10} {:>6.0}% {:>8.2} {:>7.0}% {:>8.2}% {:>7}", s, wrs, aws, dds, rts, pf_str);
    }
    println!();

    let pr = pass as f64 / nw as f64 * 100.0;
    println!("  {} Honest Assessment {}", "=".repeat(18), "=".repeat(18));
    println!();
    println!("  {} windows, {} trades", nw, tt);
    if pr >= 70.0 { println!("  VIABLE: {:.0}% pass rate meets Track C bar", pr); }
    else if pr >= 50.0 { println!("  BORDERLINE: {:.0}% pass rate — sleeve only", pr); }
    else { println!("  FAILED: {:.0}% pass rate — GRAVEYARD", pr); }
    if sh < 1.0 && pr < 70.0 { println!("  GRAVEYARD — low sh({:.2}) + low pass({:.0}%)", sh, pr); }
    let tc_sh = 3.5;
    println!();
    println!("  vs Turtle+Chandelier (87% pass, sh~{:.1} fee-adj):", tc_sh);
    println!("  MR sh {:.2} vs T+C {:.1} — {:+.0}%", sh, tc_sh, (sh - tc_sh) / tc_sh * 100.0);

    Ok(())
}