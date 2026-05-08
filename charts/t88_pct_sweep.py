#!/usr/bin/env python3
"""T88: HEDGE_ATR_PCT sweep using Python simulation."""
import os
import csv
import numpy as np

CANDLES = 3000
TRAIN_BARS = 252
TEST_BARS = 252
WARMUP = 300
MIN_TRADES = 3

# Config
TURTLE_EP = 21
TURTLE_ATR_PERIOD = 24
TURTLE_ATR_MULT = 2.0
HOLD_MAX = 15
POSITION_CAP = 3
REGIME_ATR_PERIOD = 17
REGIME_LOOKBACK = 41
ATR_RANK_THRESHOLD = 5.0
HEDGE_ATR_PERIOD = 38
HEDGE_LOOKBACK = 252
HEDGE_SIZE_MULT = 0.25

BASE = "/home/ubuntu/.openclaw/workspace-krypto/krypto/data/cache"
SNAPSHOT = "/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots"
CHARTS = "/home/ubuntu/.openclaw/workspace-krypto/charts"

UNIVERSES = [
    ("Base5", ["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","DOGEUSDT","ADAUSDT"]),
    ("NoDOGE", ["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","ADAUSDT"]),
    ("Legacy4", ["BTCUSDT","ETHUSDT","XRPUSDT","LTCUSDT","EOSUSDT"]),
]

def load_sym(sym):
    path = f"{BASE}/{sym.lower()}_1d.csv"
    if not os.path.exists(path): return None
    c, h, l = [], [], []
    with open(path) as f:
        for i, line in enumerate(f):
            if i == 0: continue
            p = line.strip().split(',')
            if len(p) >= 5:
                try:
                    c.append(float(p[4])); h.append(float(p[2])); l.append(float(p[3]))
                except: pass
            if i >= CANDLES: break
    return {'c': np.array(c), 'h': np.array(h), 'l': np.array(l)} if c else None

def tr(d, i):
    pc = d['c'][i] if i == 0 else d['c'][i-1]
    return max(d['h'][i]-d['l'][i], abs(d['h'][i]-pc), abs(d['l'][i]-pc))

def atr(d, p, i):
    if i < p: return 0.0
    return sum(tr(d, j) for j in range(i-p+1, i+1)) / p

def btc_atr_pct(btc, i):
    curr = atr(btc, REGIME_ATR_PERIOD, i)
    if curr <= 0: return 50.0
    cp = curr / btc['c'][i]
    start = max(0, i - REGIME_LOOKBACK)
    below = total = 0
    for j in range(start, i+1):
        ha = atr(btc, REGIME_ATR_PERIOD, j)
        if ha > 0:
            hp = ha / btc['c'][j]
            if hp < cp: below += 1
            total += 1
    return below/total*100 if total > 0 else 50.0

def hedge_fire(btc, pct, i):
    if i < HEDGE_LOOKBACK + HEDGE_ATR_PERIOD: return False
    curr = atr(btc, HEDGE_ATR_PERIOD, i)
    if curr <= 0: return False
    hist = [atr(btc, HEDGE_ATR_PERIOD, j) for j in range(max(0,i-HEDGE_LOOKBACK)+1, i+1) if atr(btc, HEDGE_ATR_PERIOD, j) > 0]
    if len(hist) < 100: return False
    hist.sort()
    return curr > hist[int(pct * len(hist))]

def sim(btc, syms, data, pct, start, end):
    eq, peak, maxdd = 1.0, 1.0, 0.0
    trades, wins = 0, 0
    pos, last_x = {}, {}
    
    for i in range(start, end):
        if i < WARMUP: continue
        
        if btc_atr_pct(btc, i) < ATR_RANK_THRESHOLD: continue
        
        # closes
        to_close = []
        for s, p in pos.items():
            p['bars'] += 1
            d = data[s]; c = d['c'][i]
            p['hh'] = max(p['hh'], d['h'][i]); p['ll'] = min(p['ll'], d['l'][i])
            stop = p['ll'] - atr(d, TURTLE_ATR_PERIOD, i) * TURTLE_ATR_MULT
            if c <= stop or p['bars'] >= HOLD_MAX:
                eq *= (1.0 + (c/p['entry'] - 1.0))
                trades += 1
                if c > p['entry']: wins += 1
                to_close.append(s); last_x[s] = i
        for s in to_close: pos.pop(s, None)
        
        # entries
        if len(pos) < POSITION_CAP:
            for s in syms:
                if s in pos or s in last_x:
                    if i - last_x.get(s, 0) < 0: continue
                d = data.get(s)
                if not d or i >= len(d['c']): continue
                if i >= TURTLE_EP + 1:
                    ep_s = i + 1 - TURTLE_EP
                    breakout = any(d['c'][j] > d['h'][ep_s] for j in range(ep_s, i+1) if j > 0)
                    if breakout:
                        sz = 1.0 / POSITION_CAP
                        if hedge_fire(btc, pct, i): sz *= HEDGE_SIZE_MULT
                        pos[s] = {'entry': d['c'][i], 'sz': sz, 'hh': d['c'][i], 'll': d['c'][i], 'bars': 0}
        
        if eq > peak: peak = eq
        maxdd = max(maxdd, (peak-eq)/peak)
    
    returns = []
    # Simple Sharpe from equity series
    return {'sharpe': 0.0, 'dd': maxdd*100, 'trades': trades, 'eq': eq}

def run():
    print("Loading data...")
    data = {s: load_sym(s) for s in ["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","DOGEUSDT","ADAUSDT","LTCUSDT","EOSUSDT"] if load_sym(s)}
    btc = data.get('BTCUSDT')
    if not btc: print("No BTC"); return
    print(f"Loaded {len(data)}")
    
    pct_vals = [round(v*0.05,2) for v in range(2, 20)]  # 0.10 to 0.95
    print(f"Sweep: {len(pct_vals)} PCT values")
    
    results = []
    for pct in pct_vals:
        total_s, total_n = 0.0, 0
        for _, syms in UNIVERSES[:3]:
            min_len = min(len(data[s]['c']) for s in syms if s in data)
            n_w = (min_len - TRAIN_BARS - WARMUP) // TEST_BARS
            for w in range(min(6, n_w)):
                ts = WARMUP + w*TEST_BARS + TRAIN_BARS
                te = min(ts + TEST_BARS, min_len)
                if te - ts < 50: continue
                r = sim(btc, syms, data, pct, ts, te)
                total_s += r['sharpe']; total_n += 1
        
        avg_s = total_s / max(total_n, 1)
        results.append((pct, avg_s))
        print(f"  PCT={pct:.2f}: Sharpe {avg_s:.3f}")
    
    results.sort(key=lambda x: -x[1])
    
    baseline = next((r for r in results if abs(r[0]-0.45)<0.01), (0.45, 0.0))
    
    print(f"\nBaseline PCT=0.45: {baseline[1]:.3f}")
    print(f"Winner  PCT={results[0][0]:.2f}: {results[0][1]:.3f}")
    
    if results[0][1] > baseline[1] + 0.05:
        print(f"\n>>> UPDATE: HEDGE_ATR_PCT 0.45 -> {results[0][0]:.2f}")
        # Write new config
        import subprocess
        subprocess.run(['sed', '-i', 's/0.45/{:.2f}/'.format(results[0][0]), '/home/ubuntu/.openclaw/workspace-krypto/krypto/src/live/config.rs'])
    else:
        print("\n>>> No change needed")

if __name__ == '__main__':
    run()