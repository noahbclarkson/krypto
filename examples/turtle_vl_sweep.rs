// Quick VOL_LOOKBACK sweep for Turtle-only (9 universes, 6 windows)
use anyhow::Result;
use krypto::data::loader::DataLoader;
use polars::prelude::*;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::time::Instant;

const CANDLES: u32 = 3000; const TRAIN_BARS: usize = 252; const TEST_BARS: usize = 252;
const HOLD_MAX: usize = 12; const TAKER_FEE: f64 = 0.001; const POSITION_CAP: usize = 3;
const MIN_TRADES: usize = 3; const TURTLE_ENTRY: usize = 21;
const TURTLE_ATR_PERIOD: usize = 24; const TURTLE_ATR_MULT: f64 = 2.00; const ATR_ENTRY_MULT: f64 = 0.00;

const VL_VALUES: &[usize] = &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 12, 15, 20, 30, 50, 100];

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

struct SymData { close: Vec<f64>, high: Vec<f64>, low: Vec<f64>, vol: Vec<f64> }
struct WinSetup { sym_data: HashMap<String, SymData>, symbols: Vec<String>, windows: Vec<(usize,usize)> }

fn atr_at(h: &[f64], l: &[f64], c: &[f64], p: usize, i: usize) -> f64 {
    if i < p { return 0.0; }
    let mut trs = Vec::with_capacity(p);
    for j in (i+1-p)..=i { let h0=h[j]; let l0=l[j]; let c0=c[j.saturating_sub(1)];
        trs.push((h0-l0).max((h0-c0).abs()).max((l0-c0).abs())); }
    trs.iter().sum::<f64>() / p as f64
}
fn ravg(v: &[f64], w: usize, i: usize) -> f64 {
    if i < w { v[i] } else { v[i+1-w..=i].iter().sum::<f64>() / w as f64 }
}
fn turtle_sig(c: &[f64], h: &[f64], l: &[f64], ep: usize, ap: usize, am: f64, i: usize) -> bool {
    if i < ep+1 { return false; }
    let mut mx = f64::NEG_INFINITY;
    for j in i+1-ep..i { if let Some(&cv) = c.get(j) { mx = mx.max(cv); } }
    if let Some(&cc) = c.get(i) {
        if cc > mx && am > 0.0 { return cc >= mx + am * atr_at(h,l,c,ap,i); }
        return cc > mx;
    }
    false
}
fn ash(d: &[f64]) -> f64 {
    if d.len()<2 { return 0.0; }
    let mn=d.iter().sum::<f64>()/d.len() as f64;
    let sd=(d.iter().map(|x|(x-mn).powi(2)).sum::<f64>()/d.len() as f64).sqrt();
    if sd==0.0 { return 0.0; }
    mn*365.0_f64.sqrt()/sd
}

fn run_sim(sd: &HashMap<String,SymData>, syms: &[String], ts: usize, te: usize, vl: usize) -> (f64,f64,usize,bool) {
    let mut eq = 1.0_f64; let mut wins=0usize; let mut trades=0usize; let mut dr = Vec::new();
    let mut bar = ts;
    while bar+2<te {
        let mut sc: Vec<(&str,f64)> = Vec::new();
        for sym in syms {
            if let Some(s) = sd.get(sym) { if bar>=s.close.len() { continue; }
                let rv = ravg(&s.vol, vl, bar); let px = s.close[bar]; let dv = rv*px;
                sc.push((sym.as_str(), if dv.is_finite()&&dv>0.0{dv}else{0.0}));
            }
        }
        sc.sort_by(|a,b| b.1.partial_cmp(&a.1).unwrap());
        let top: Vec<String> = sc.into_iter().take(POSITION_CAP).map(|(s,_)|s.to_string()).collect();
        if top.is_empty() { bar+=1; continue; }
        let mut entered = false;
        for sym in &top {
            if let Some(s) = sd.get(sym) {
                if bar>=TURTLE_ENTRY+1 && bar<s.close.len() {
                    if turtle_sig(&s.close,&s.high,&s.low,TURTLE_ENTRY,TURTLE_ATR_PERIOD,ATR_ENTRY_MULT,bar) {
                        let ep = s.close[bar]*(1.0-TAKER_FEE); let en = bar+1; let nn = s.close.len();
                        let mut ll = s.low[en]; let mb = (en+HOLD_MAX).min(nn.saturating_sub(1)); let mut eb = mb;
                        for b in en..=mb { ll = ll.min(s.low[b]);
                            let at = atr_at(&s.high,&s.low,&s.close,TURTLE_ATR_PERIOD,b);
                            let tr = ll - TURTLE_ATR_MULT*at;
                            if s.close[b]<tr { eb=b; break; } }
                        if let Some(&xp) = s.close.get(eb) {
                            let ex = xp*(1.0-TAKER_FEE); let gr = ex/ep-1.0;
                            let bh = (eb as i64-en as i64).max(1) as usize;
                            wins += if gr>0.0{1}else{0}; trades+=1; eq*=1.0+gr;
                            let ad = gr/bh as f64; for _ in 0..bh{dr.push(ad);}
                            bar=eb+1; entered=true; break;
                        }
                    }
                }
            }
        }
        if !entered { bar+=1; }
    }
    let ret=(eq-1.0)*100.0; let sh=ash(&dr); let pass=trades>=MIN_TRADES&&ret>0.0;
    (ret,sh,trades,pass)
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    let loader = DataLoader::new(None,None);
    let mut all: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (_,ss) in UNIVERSES { for &s in *ss { all.insert(s.to_string()); } }
    let mut cache: HashMap<String,DataFrame> = HashMap::new(); let mut min = usize::MAX;
    for sym in &all {
        match loader.fetch_with_cache(sym,"1d",CANDLES).await {
            Ok(df) => { min=min.min(df.height()); cache.insert(sym.clone(),df); }
            Err(e) => eprintln!("WARN {}: {}",sym,e)
        }
    }
    let n = min.min(2800); eprintln!("Loaded {} syms, {} bars",cache.len(),n);
    let mut m: HashMap<String,SymData> = HashMap::new();
    for sym in &all {
        if let Some(df) = cache.get(sym) {
            let n2 = df.height().min(n);
            macro_rules! cv { ($n:expr) => {{ let ch=df.column($n)?.f64()?;
                ch.into_iter().filter_map(|x|x).take(n2).collect::<Vec<_>>() }};
            }
            m.insert(sym.clone(), SymData{close:cv!("close"),high:cv!("high"),low:cv!("low"),vol:cv!("volume")});
        }
    }
    let mut setups: Vec<WinSetup> = Vec::new();
    for &(un,ss) in UNIVERSES {
        let syms: Vec<String> = ss.iter().map(|s|s.to_string()).collect();
        if !syms.iter().all(|s|m.contains_key(s)) { continue; }
        let tw = n.saturating_sub(TRAIN_BARS+TEST_BARS)/TEST_BARS;
        if tw==0 { continue; }
        let sd: HashMap<String,SymData> = syms.iter().filter_map(|s| m.get(s).map(|v| (s.to_string(),SymData{close:v.close.clone(),high:v.high.clone(),low:v.low.clone(),vol:v.vol.clone()}))).collect();
        let wins: Vec<(usize,usize)> = (0..tw).filter_map(|wi| {
            let te=TRAIN_BARS+wi*TEST_BARS; let ts=te; let tn=(ts+TEST_BARS).min(n);
            if tn-ts<5{None}else{Some((ts,tn))} }).collect();
        setups.push(WinSetup{sym_data:sd,symbols:syms,windows:wins});
    }
    let mut csv = vec!("vl,pass_rate,total_pass,total_windows,pos_unis,avg_sharpe,avg_ret,total_trades".to_string());
    eprintln!("\n{:>4} | {:>5} | {:>5} | {:>9} | {:>8} | {:>6}","VL","Pass","PosU","AvgSharpe","AvgRet%","Trades");
    eprintln!("{}", "-".repeat(42));
    for &vl in VL_VALUES {
        let t1 = Instant::now();
        let mut tp=0usize; let mut tt=0usize; let mut ss=0.0_f64; let mut sr=0.0_f64; let mut st=0usize; let mut pu=0usize;
        for s in &setups {
            let mut up=false;
            for &(ts,te) in &s.windows {
                let (r,sh,tr,pa) = run_sim(&s.sym_data,&s.symbols,ts,te,vl);
                tp+=if pa{1}else{0}; tt+=1; ss+=sh; sr+=r; st+=tr;
                if r>0.0{up=true;}
            }
            if up{pu+=1;}
        }
        let pr=if tt>0{tp as f64/tt as f64*100.0}else{0.0};
        let ash=if tt>0{ss/tt as f64}else{0.0};
        let ar=if tt>0{sr/tt as f64}else{0.0};
        csv.push(format!("{},{:.2},{:.0},{:.0},{},{:.4},{:.2},{}",vl,pr,tp,tt,pu,ash,ar,st));
        eprintln!("VL={:>3}: {:>5.0}% pass, {} pos uni, Sharpe={:.3}, Ret={:+7.1}%, {} trades [{:.1}s]",vl,pr,pu,ash,ar,st,t1.elapsed().as_secs_f64());
    }
    let mut f = File::create("snapshots/turtle_vl_sweep.csv")?;
    for r in csv { writeln!(f,"{}",r)?; }
    eprintln!("\nWrote: snapshots/turtle_vl_sweep.csv ({:.1}s)",t0.elapsed().as_secs_f64());
    Ok(())
}
