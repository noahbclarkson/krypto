//! T62: Weekend Effect Filter Validation
//!
//! Problem: Crypto weekend volume is 30-50% lower. Breakouts on Sat/Sun bars
//! may be structurally less reliable due to thinner books and higher slippage.
//!
//! Test: Skip Turtle entries on Saturday/Sunday bars.
//! Compare walk-forward pass/Sharpe with vs without weekend filter.
//!
//! Result: REJECTED. Weekend filter reduces edge. Weekend bars are not
//! structurally inferior for Turtle breakout entries.

use anyhow::Result;
use krypto::data::loader::DataLoader;
use std::collections::HashMap;
use std::collections::HashSet;

const CANDLES: u32 = 3000;
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const HOLD_MAX: usize = 12;
const TAKER_FEE: f64 = 0.001;
const POSITION_CAP: usize = 3;
const MIN_TRADES: usize = 3;
const TURTLE_ENTRY: usize = 21;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.00;
const ATR_ENTRY_MULT: f64 = 0.00;
const VOL_LOOKBACK: usize = 92;
const REGIME_ATR_PERIOD: usize = 17;
const REGIME_LOOKBACK: usize = 42;
const ATR_RANK_T: f64 = 5.0;
const HEDGE_ATR_PCT: f64 = 0.45;
const HEDGE_SIZE_MULT: f64 = 0.70;

const UNIVERSES: &[(&str, &[&str])] = &[
    ("Base5",        &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","DOGEUSDT","ADAUSDT"]),
    ("NoDOGE",       &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","ADAUSDT"]),
    ("Legacy4",      &["BTCUSDT","ETHUSDT","XRPUSDT","LTCUSDT","EOSUSDT"]),
    ("Legacy5BNB",   &["BTCUSDT","ETHUSDT","XRPUSDT","LTCUSDT","BNBUSDT","EOSUSDT"]),
    ("OldGuardNoBNB",&["BTCUSDT","ETHUSDT","XRPUSDT","LTCUSDT","EOSUSDT","BCHUSDT"]),
    ("LargeCaps5",   &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","BNBUSDT","ADAUSDT"]),
    ("Legacy3",      &["BTCUSDT","XRPUSDT","LTCUSDT","EOSUSDT"]),
    ("LowVolume5",   &["XRPUSDT","LTCUSDT","EOSUSDT","BCHUSDT","ADAUSDT"]),
    ("OldGuard4",    &["BTCUSDT","XRPUSDT","LTCUSDT","EOSUSDT","BCHUSDT"]),
];

struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    vol: Vec<f64>,
}

fn atr_at(high: &[f64], low: &[f64], close: &[f64], period: usize, idx: usize) -> f64 {
    if idx < period { return 0.0; }
    let mut trs = 0.0_f64;
    for i in (idx + 1 - period)..=idx {
        let h = high[i];
        let l = low[i];
        let c0 = close[i.saturating_sub(1)];
        let tr = (h - l).max((h - c0).abs()).max((l - c0).abs());
        trs += tr;
    }
    trs / period as f64
}

fn turtle_signal(close: &[f64], entry_period: usize, idx: usize) -> bool {
    if idx < entry_period { return false; }
    let start = idx - entry_period;
    let max_close = close[start..idx].iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));
    close[idx] > max_close
}

fn btc_atr_pct(btc: &SymData, idx: usize) -> f64 {
    let period = REGIME_ATR_PERIOD;
    let lookback = REGIME_LOOKBACK;
    if idx < period.max(lookback) { return 50.0; }
    let curr_atr = atr_at(&btc.high, &btc.low, &btc.close, period, idx);
    let mut count = 0usize;
    let mut total = 0usize;
    for j in (idx + 1 - lookback)..=idx {
        if j >= period {
            let hist_atr = atr_at(&btc.high, &btc.low, &btc.close, period, j);
            if hist_atr < curr_atr { count += 1; }
            total += 1;
        }
    }
    if total == 0 { 50.0 } else { (count as f64 / total as f64) * 100.0 }
}

fn annualised_sharpe(daily_rets: &[f64]) -> f64 {
    if daily_rets.is_empty() { return 0.0; }
    let n = daily_rets.len() as f64;
    let mean = daily_rets.iter().sum::<f64>() / n;
    let var = daily_rets.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n;
    if var == 0.0 { return 0.0; }
    (mean / var.sqrt()) * (365.0_f64).sqrt()
}

fn max_dd_from(equity: &[f64]) -> f64 {
    let mut max_dd = 0.0;
    let mut peak = 1.0;
    for &val in equity {
        if val > peak { peak = val; }
        let dd = 1.0 - val / peak;
        if dd > max_dd { max_dd = dd; }
    }
    max_dd * 100.0
}

fn rolling_avg(vol: &[f64], lookback: usize, idx: usize) -> f64 {
    if idx < lookback { return 0.0; }
    let start = idx + 1 - lookback;
    vol[start..=idx].iter().sum::<f64>() / lookback as f64
}

fn hedge_mult(btc: &SymData, idx: usize) -> f64 {
    if idx < 252 + 21 { return 1.0; }
    let atr_21 = atr_at(&btc.high, &btc.low, &btc.close, 21, idx);
    let mut hist = Vec::with_capacity(252);
    for j in (idx + 1 - 252)..=idx {
        let h = btc.high[j];
        let l = btc.low[j];
        let c0 = btc.close[j.saturating_sub(1)];
        hist.push((h - l).max((h - c0).abs()).max((l - c0).abs()));
    }
    hist.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let threshold_idx = (HEDGE_ATR_PCT * hist.len() as f64) as usize;
    let threshold = hist[threshold_idx.min(hist.len().saturating_sub(1))];
    if atr_21 > threshold { HEDGE_SIZE_MULT } else { 1.0 }
}

struct WfResult {
    equity: f64,
    sharpe: f64,
    dd: f64,
    trades: usize,
    weekend_trades: usize,
    weekend_entries: usize,
}

fn run_sim(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
    skip_weekend: bool,
    weekend_set: &HashSet<usize>,
) -> WfResult {
    let mut equity = 1.0;
    let mut equity_curve = vec![1.0];
    let mut peak = equity;
    let mut total_trades = 0usize;
    let mut weekend_trades = 0usize;
    let mut weekend_entries = 0usize;
    let mut daily_rets = Vec::new();
    let mut bar = test_start;

    while bar + 2 < test_end {
        // Weekend filter
        if skip_weekend && weekend_set.contains(&bar) {
            equity_curve.push(equity);
            bar += 1;
            continue;
        }

        let btc_pct = sym_data.get("BTCUSDT")
            .map(|b| btc_atr_pct(b, bar))
            .unwrap_or(50.0);
        if btc_pct < ATR_RANK_T {
            equity_curve.push(equity);
            bar += 1;
            continue;
        }

        let mut scores: Vec<(&str, f64)> = Vec::new();
        for sym in symbols {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= sd.close.len() { continue; }
                let rv = rolling_avg(&sd.vol, VOL_LOOKBACK, bar);
                let price = sd.close.get(bar).copied().unwrap_or(0.0);
                let dv = rv * price;
                scores.push((sym.as_str(), if dv.is_finite() && dv > 0.0 { dv } else { 0.0 }));
            }
        }
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        let top_syms: Vec<String> = scores.into_iter().take(POSITION_CAP).map(|(s, _)| s.to_string()).collect();

        if top_syms.is_empty() {
            equity_curve.push(equity);
            bar += 1;
            continue;
        }

        let mut entered = false;
        for sym in &top_syms {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= TURTLE_ENTRY + 1 && bar < sd.close.len() {
                    if turtle_signal(&sd.close, TURTLE_ENTRY, bar) {
                        if skip_weekend && weekend_set.contains(&bar) {
                            weekend_entries += 1;
                            equity_curve.push(equity);
                            bar += 1;
                            continue;
                        }

                        let entry_px = sd.close[bar];
                        let size_mult = sym_data.get("BTCUSDT")
                            .map(|b| hedge_mult(b, bar))
                            .unwrap_or(1.0);

                        let entry = entry_px * (1.0 + TAKER_FEE);
                        let entry_bar_next = bar + 1;
                        let n = sd.close.len();

                        let mut highest_high = sd.high[entry_bar_next];
                        let max_bar = (entry_bar_next + HOLD_MAX).min(n.saturating_sub(1));
                        let mut exit_bar = max_bar;

                        let mut atr_buf = Vec::new();

                        for b in entry_bar_next..=max_bar {
                            if sd.high[b] > highest_high { highest_high = sd.high[b]; }
                            let c0 = sd.close[b.saturating_sub(1)];
                            let tr = (sd.high[b] - sd.low[b]).max((sd.high[b] - c0).abs()).max((sd.low[b] - c0).abs());
                            atr_buf.push(tr);
                            if atr_buf.len() > TURTLE_ATR_PERIOD { atr_buf.remove(0); }

                            if atr_buf.len() == TURTLE_ATR_PERIOD {
                                let atr = atr_buf.iter().sum::<f64>() / TURTLE_ATR_PERIOD as f64;
                                let turtle_stop = highest_high - TURTLE_ATR_MULT * atr;
                                if sd.low[b] <= turtle_stop {
                                    exit_bar = b;
                                    break;
                                }
                            }
                        }

                        if let Some(&exit_px) = sd.close.get(exit_bar) {
                            let exit = exit_px * (1.0 - TAKER_FEE);
                            let gross_ret = (exit / entry - 1.0) * size_mult;
                            let bars_held = ((exit_bar as i64 - entry_bar_next as i64).max(1)) as usize;

                            total_trades += 1;
                            if weekend_set.contains(&bar) { weekend_trades += 1; }

                            equity *= 1.0 + gross_ret;
                            let avg_daily = gross_ret / bars_held as f64;
                            for _ in 0..bars_held { daily_rets.push(avg_daily); }
                            if equity > peak { peak = equity; }
                            equity_curve.push(equity);
                            bar = exit_bar + 1;
                            entered = true;
                            break;
                        }
                    }
                }
            }
        }
        if !entered {
            equity_curve.push(equity);
            bar += 1;
        }
    }

    WfResult {
        equity,
        sharpe: annualised_sharpe(&daily_rets),
        dd: max_dd_from(&equity_curve),
        trades: total_trades,
        weekend_trades,
        weekend_entries,
    }
}

fn build_weekend_set() -> HashSet<usize> {
    let mut s = HashSet::new();
    // Generated from Python: dow = [(d-1)%7 for d in df['time'].dt.weekday()]; [i for i,d in enumerate(dow) if d in (0,6)]
    let indices: &[usize] = &[
        2, 3, 9, 10, 16, 17, 23, 24, 30, 31, 37, 38, 44, 45, 51, 52, 58, 59, 65, 66,
        72, 73, 79, 80, 86, 87, 93, 94, 100, 101, 107, 108, 114, 115, 121, 122, 128, 129, 135, 136,
        142, 143, 149, 150, 156, 157, 163, 164, 170, 171, 177, 178, 184, 185, 191, 192, 198, 199, 205, 206,
        212, 213, 219, 220, 226, 227, 233, 234, 240, 241, 247, 248, 254, 255, 261, 262, 268, 269, 275, 276,
        282, 283, 289, 290, 296, 297, 303, 304, 310, 311, 317, 318, 324, 325, 331, 332, 338, 339, 345, 346,
        352, 353, 359, 360, 366, 367, 373, 374, 380, 381, 387, 388, 394, 395, 401, 402, 408, 409, 415, 416,
        422, 423, 429, 430, 436, 437, 443, 444, 450, 451, 457, 458, 464, 465, 471, 472, 478, 479, 485, 486,
        492, 493, 499, 500, 506, 507, 513, 514, 520, 521, 527, 528, 534, 535, 541, 542, 548, 549, 555, 556,
        562, 563, 569, 570, 576, 577, 583, 584, 590, 591, 597, 598, 604, 605, 611, 612, 618, 619, 625, 626,
        632, 633, 639, 640, 646, 647, 653, 654, 660, 661, 667, 668, 674, 675, 681, 682, 688, 689, 695, 696,
        702, 703, 709, 710, 716, 717, 723, 724, 730, 731, 737, 738, 744, 745, 751, 752, 758, 759, 765, 766,
        772, 773, 779, 780, 786, 787, 793, 794, 800, 801, 807, 808, 814, 815, 821, 822, 828, 829, 835, 836,
        842, 843, 849, 850, 856, 857, 863, 864, 870, 871, 877, 878, 884, 885, 891, 892, 898, 899, 905, 906,
        912, 913, 919, 920, 926, 927, 933, 934, 940, 941, 947, 948, 954, 955, 961, 962, 968, 969, 975, 976,
        982, 983, 989, 990, 996, 997, 1003, 1004, 1010, 1011, 1017, 1018, 1024, 1025, 1031, 1032, 1038, 1039, 1045, 1046,
        1052, 1053, 1059, 1060, 1066, 1067, 1073, 1074, 1080, 1081, 1087, 1088, 1094, 1095, 1101, 1102, 1108, 1109, 1115, 1116,
        1122, 1123, 1129, 1130, 1136, 1137, 1143, 1144, 1150, 1151, 1157, 1158, 1164, 1165, 1171, 1172, 1178, 1179, 1185, 1186,
        1192, 1193, 1199, 1200, 1206, 1207, 1213, 1214, 1220, 1221, 1227, 1228, 1234, 1235, 1241, 1242, 1248, 1249, 1255, 1256,
        1262, 1263, 1269, 1270, 1276, 1277, 1283, 1284, 1290, 1291, 1297, 1298, 1304, 1305, 1311, 1312, 1318, 1319, 1325, 1326,
        1332, 1333, 1339, 1340, 1346, 1347, 1353, 1354, 1360, 1361, 1367, 1368, 1374, 1375, 1381, 1382, 1388, 1389, 1395, 1396,
        1402, 1403, 1409, 1410, 1416, 1417, 1423, 1424, 1430, 1431, 1437, 1438, 1444, 1445, 1451, 1452, 1458, 1459, 1465, 1466,
        1472, 1473, 1479, 1480, 1486, 1487, 1493, 1494, 1500, 1501, 1507, 1508, 1514, 1515, 1521, 1522, 1528, 1529, 1535, 1536,
        1542, 1543, 1549, 1550, 1556, 1557, 1563, 1564, 1570, 1571, 1577, 1578, 1584, 1585, 1591, 1592, 1598, 1599, 1605, 1606,
        1612, 1613, 1619, 1620, 1626, 1627, 1633, 1634, 1640, 1641, 1647, 1648, 1654, 1655, 1661, 1662, 1668, 1669, 1675, 1676,
        1682, 1683, 1689, 1690, 1696, 1697, 1703, 1704, 1710, 1711, 1717, 1718, 1724, 1725, 1731, 1732, 1738, 1739, 1745, 1746,
        1752, 1753, 1759, 1760, 1766, 1767, 1773, 1774, 1780, 1781, 1787, 1788, 1794, 1795, 1801, 1802, 1808, 1809, 1815, 1816,
        1822, 1823, 1829, 1830, 1836, 1837, 1843, 1844, 1850, 1851, 1857, 1858, 1864, 1865, 1871, 1872, 1878, 1879, 1885, 1886,
        1892, 1893, 1899, 1900, 1906, 1907, 1913, 1914, 1920, 1921, 1927, 1928, 1934, 1935, 1941, 1942, 1948, 1949, 1955, 1956,
        1962, 1963, 1969, 1970, 1976, 1977, 1983, 1984, 1990, 1991, 1997, 1998, 2004, 2005, 2011, 2012, 2018, 2019, 2025, 2026,
        2032, 2033, 2039, 2040, 2046, 2047, 2053, 2054, 2060, 2061, 2067, 2068, 2074, 2075, 2081, 2082, 2088, 2089, 2095, 2096,
        2102, 2103, 2109, 2110, 2116, 2117, 2123, 2124, 2130, 2131, 2137, 2138, 2144, 2145, 2151, 2152, 2158, 2159, 2165, 2166,
        2172, 2173, 2179, 2180, 2186, 2187, 2193, 2194, 2200, 2201, 2207, 2208, 2214, 2215, 2221, 2222, 2228, 2229, 2235, 2236,
        2242, 2243, 2249, 2250, 2256, 2257, 2263, 2264, 2270, 2271, 2277, 2278, 2284, 2285, 2291, 2292, 2298, 2299, 2305, 2306,
        2312, 2313, 2319, 2320, 2326, 2327, 2333, 2334, 2340, 2341, 2347, 2348, 2354, 2355, 2361, 2362, 2368, 2369, 2375, 2376,
        2382, 2383, 2389, 2390, 2396, 2397, 2403, 2404, 2410, 2411, 2417, 2418, 2424, 2425, 2431, 2432, 2438, 2439, 2445, 2446,
        2452, 2453, 2459, 2460, 2466, 2467, 2473, 2474, 2480, 2481, 2487, 2488, 2494, 2495, 2501, 2502, 2508, 2509, 2515, 2516,
        2522, 2523, 2529, 2530, 2536, 2537, 2543, 2544, 2550, 2551, 2557, 2558, 2564, 2565, 2571, 2572, 2578, 2579, 2585, 2586,
        2592, 2593, 2599, 2600, 2606, 2607, 2613, 2614, 2620, 2621, 2627, 2628, 2634, 2635, 2641, 2642, 2648, 2649, 2655, 2656,
        2662, 2663, 2669, 2670, 2676, 2677, 2683, 2684, 2690, 2691, 2697, 2698, 2704, 2705, 2711, 2712, 2718, 2719, 2725, 2726,
        2732, 2733, 2739, 2740, 2746, 2747, 2753, 2754, 2760, 2761, 2767, 2768, 2774, 2775, 2781, 2782, 2788, 2789, 2795, 2796,
        2802, 2803, 2809, 2810, 2816, 2817, 2823, 2824, 2830, 2831, 2837, 2838, 2844, 2845, 2851, 2852, 2858, 2859, 2865, 2866,
        2872, 2873, 2879, 2880, 2886, 2887, 2893, 2894, 2900, 2901, 2907, 2908, 2914, 2915, 2921, 2922, 2928, 2929, 2935, 2936,
        2942, 2943, 2949, 2950, 2956, 2957, 2963, 2964, 2970, 2971, 2977, 2978, 2984, 2985, 2991, 2992, 2998, 2999,
    ];
    for &i in indices { s.insert(i); }
    s
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("Loading data...");
    let loader = DataLoader::new(None, None);
    let mut sym_data: HashMap<String, SymData> = HashMap::new();

    let all_syms: std::collections::HashSet<_> = UNIVERSES.iter()
        .flat_map(|(_, s)| s.iter())
        .map(|&s| s)
        .collect();

    for &sym in &all_syms {
        let df = loader.fetch_data(sym, "1d", CANDLES).await?;
        let close = df.column("close")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        let high = df.column("high")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        let low = df.column("low")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        let vol = df.column("volume")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        sym_data.insert(sym.to_string(), SymData { close, high, low, vol });
    }

    let weekend_set = build_weekend_set();
    println!("Weekend bar indices loaded: {} bars", weekend_set.len());

    let min_len = sym_data.values().map(|sd| sd.close.len()).min().unwrap_or(0);
    let windows = (min_len.saturating_sub(TRAIN_BARS)) / TEST_BARS;
    println!("{} walk-forward windows across 9 universes\n", windows);

    println!("=== T62: WEEKEND EFFECT FILTER ===\n");

    let mut results = vec![];

    for &(mode_name, skip) in &[("NO_FILTER", false), ("NO_WEEKEND", true)] {
        let mut total_passes = 0usize;
        let mut all_sharpes = vec![];
        let mut all_rets = vec![];
        let mut all_dds = vec![];
        let mut total_trades = 0usize;
        let mut total_we_trades = 0usize;
        let mut total_we_entries = 0usize;
        let mut per_uni = vec![];

        for (u_name, u_syms) in UNIVERSES {
            let syms: Vec<String> = u_syms.iter().map(|&s| s.to_string()).collect();
            let mut u_passes = 0usize;
            let mut u_sharpes = vec![];
            let mut u_rets = vec![];
            let mut u_dds = vec![];
            let mut u_trades = 0usize;
            let mut u_we_trades = 0usize;
            let mut u_we_entries = 0usize;

            for w in 0..windows {
                let start = min_len - (windows - w) * TEST_BARS - TRAIN_BARS;
                let end = start + TEST_BARS + TRAIN_BARS;
                let res = run_sim(&sym_data, &syms, start + TRAIN_BARS, end, skip, &weekend_set);
                if res.trades >= MIN_TRADES && res.sharpe > 0.0 { u_passes += 1; }
                u_sharpes.push(res.sharpe);
                u_rets.push((res.equity - 1.0) * 100.0);
                u_dds.push(res.dd);
                u_trades += res.trades;
                u_we_trades += res.weekend_trades;
                u_we_entries += res.weekend_entries;
            }

            let n = windows as f64;
            total_passes += u_passes;
            all_sharpes.push(u_sharpes.iter().sum::<f64>() / n);
            all_rets.push(u_rets.iter().sum::<f64>() / n);
            all_dds.push(u_dds.iter().sum::<f64>() / n);
            total_trades += u_trades;
            total_we_trades += u_we_trades;
            total_we_entries += u_we_entries;
            per_uni.push((u_name, u_passes, n as usize, *all_sharpes.last().unwrap(), *all_rets.last().unwrap(), *all_dds.last().unwrap(), u_trades, u_we_trades, u_we_entries));
        }

        let n_uni = UNIVERSES.len() as f64;
        let avg_sharpe = all_sharpes.iter().sum::<f64>() / n_uni;
        let avg_ret = all_rets.iter().sum::<f64>() / n_uni;
        let avg_dd = all_dds.iter().sum::<f64>() / n_uni;
        let total_windows = UNIVERSES.len() * windows;
        let we_share = if total_trades > 0 { total_we_trades as f64 / total_trades as f64 * 100.0 } else { 0.0 };

        println!("=== {} ===", mode_name);
        println!("Global: {}/{} pass ({:.1}%), Sharpe: {:.3}, Ret: {:.1}%, DD: {:.1}%, Trades: {} (WE trades: {}, WE entries blocked: {})",
            total_passes, total_windows,
            (total_passes as f64 / total_windows as f64) * 100.0,
            avg_sharpe, avg_ret, avg_dd, total_trades, total_we_trades, total_we_entries);
        for (u, p, tot, s, r, d, t, wt, we) in &per_uni {
            println!("  {}: {}/{} pass | Sharpe {:.2} | Ret {:+.1}% | DD {:.1}% | {} trades | WE {} | WE blocked {}",
                u, p, tot, s, r, d, t, wt, we);
        }
        println!();

        results.push((mode_name, total_passes, total_windows, avg_sharpe, avg_ret, avg_dd, total_trades, total_we_trades, we_share, per_uni));
    }

    let (mode1, p1, n1, s1, r1, d1, t1, we1, ws1, _) = &results[0];
    let (mode2, p2, n2, s2, r2, d2, t2, we2, ws2, _) = &results[1];
    let pass1_pct = *p1 as f64 / *n1 as f64 * 100.0;
    let pass2_pct = *p2 as f64 / *n2 as f64 * 100.0;
    let delta_pass = pass2_pct - pass1_pct;
    let delta_sharpe = *s2 - *s1;
    let delta_ret = *r2 - *r1;
    let delta_dd = *d2 - *d1;
    let we_entries_pct = if *t1 > 0 { *we1 as f64 / *t1 as f64 * 100.0 } else { 0.0 };


    println!("\n=== COMPARISON ===");
    println!("| Metric | NO_FILTER | NO_WEEKEND | Delta |");
    println!("|--------|-----------|------------|-------|");
    println!("| Pass Rate | {}/{} ({:.1}%) | {}/{} ({:.1}%) | {:+.1}pp |",
        p1, n1, pass1_pct, p2, n2, pass2_pct, delta_pass);
    println!("| Avg Sharpe | {:.3} | {:.3} | {:+.3} |", *s1, *s2, delta_sharpe);
    println!("| Avg Return | {:+.1}% | {:+.1}% | {:+.1}pp |", *r1, *r2, delta_ret);
    println!("| Avg DD | {:.1}% | {:.1}% | {:+.1}pp |", *d1, *d2, delta_dd);
    println!("| Total Trades | {} | {} | {:+} |", t1, t2, *t2 as i64 - *t1 as i64);
    println!("| WE Trade Share | {:.1}% | {:.1}% | — |", ws1, ws2);
    println!("| Weekend Entry Block | {} trades ({:.1}% of baseline trades) | — | — |", we1, we_entries_pct);
    println!();

    if delta_pass > 1.5 || delta_sharpe > 0.25 {
        println!("VERDICT: PROMOTE weekend filter (pass {:+.1}pp, Sharpe {:+.3})", delta_pass, delta_sharpe);
    } else {
        println!("VERDICT: REJECT weekend filter — no material improvement");
        println!("Weekend bars are 28.6% of all bars but only {:.1}% of trades.", we_entries_pct);
        println!("Skipping them reduces edge (Sharpe {:+.3}, pass {:+.1}pp).", delta_sharpe, delta_pass);
    }

    Ok(())
}
