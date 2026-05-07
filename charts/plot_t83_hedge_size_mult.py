#!/usr/bin/env python3
"""Plot T83 HEDGE_SIZE_MULT Base5 equity curves."""
from pathlib import Path
import csv
import matplotlib.pyplot as plt

ROOT = Path('/home/ubuntu/.openclaw/workspace-krypto')
SNAP = ROOT / 'krypto' / 'snapshots'
OUT = ROOT / 'charts' / 'comparison_chart.png'
SUMMARY = SNAP / 't83_hedge_size_mult_summary.csv'
EQUITY = SNAP / 't83_hedge_size_mult_equity.csv'

# Robustness order: pass rate desc, avg sharpe desc.
rows = []
with SUMMARY.open() as f:
    for row in csv.DictReader(f):
        rows.append(row)
rows.sort(key=lambda r: (float(r['pass_rate_pct']), float(r['avg_sharpe'])), reverse=True)

baseline = 0.55
winner = float(rows[0]['hedge_size_mult'])
selected = [baseline, winner]
for r in rows:
    v = float(r['hedge_size_mult'])
    if v not in selected:
        selected.append(v)
    if len(selected) >= 5:
        break
selected = sorted(set(selected), key=selected.index)

curves = {v: {'step': [], 'equity': []} for v in selected}
with EQUITY.open() as f:
    for row in csv.DictReader(f):
        v = round(float(row['hedge_size_mult']), 2)
        if v in curves:
            curves[v]['step'].append(int(row['step']))
            curves[v]['equity'].append(float(row['equity']))

plt.figure(figsize=(14, 8), dpi=160)
colors = ['#d62728', '#2ca02c', '#1f77b4', '#ff7f0e', '#9467bd']
all_eq = []
summary_by_val = {round(float(r['hedge_size_mult']),2): r for r in rows}
for i, v in enumerate(selected):
    c = curves[v]
    if not c['equity']:
        continue
    all_eq.extend(c['equity'])
    label = f"HSM={v:.2f}"
    if abs(v - baseline) < 1e-9:
        label += " baseline"
    if abs(v - winner) < 1e-9:
        label += " winner"
    sr = summary_by_val.get(round(v,2))
    if sr:
        label += f" ({sr['pass_count']}/{sr['total_windows']} pass, Sh {float(sr['avg_sharpe']):.2f})"
    plt.plot(c['step'], c['equity'], linewidth=2.0, color=colors[i % len(colors)], label=label)

if all_eq:
    lo, hi = min(all_eq), max(all_eq)
    pad = max((hi - lo) * 0.08, 0.02)
    plt.ylim(max(0.0, lo - pad), hi + pad)

plt.title('T83 HEDGE_SIZE_MULT optimization — Base5 exact-live equity curves', fontsize=15)
plt.xlabel('Step / daily bar since warmup')
plt.ylabel('Portfolio equity multiple')
plt.grid(True, alpha=0.35)
plt.legend(loc='best', fontsize=9)
plt.tight_layout()
OUT.parent.mkdir(parents=True, exist_ok=True)
plt.savefig(OUT)
print(f'Wrote {OUT}')
print('Plotted:', ', '.join(f'{v:.2f}' for v in selected))
