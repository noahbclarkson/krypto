import csv
import numpy as np
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
from collections import defaultdict

files = {
    'Baseline\n(thresh=100, sm=1.0)': '/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots/usdt_hedge_equity_baseline.csv',
    'Winner\n(thresh=55, sm=0.55)': '/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots/usdt_hedge_equity_winner.csv',
    'Runner-up 1\n(thresh=50, sm=0.55)': '/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots/usdt_hedge_equity_runnerup1.csv',
    'Runner-up 2\n(thresh=45, sm=0.50)': '/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots/usdt_hedge_equity_runnerup2.csv',
}

fig, (ax1, ax2) = plt.subplots(2, 1, figsize=(14, 10))

colors = ['#888888', '#2E86AB', '#E94F37', '#1BCD9E']

# Plot mean equity per bar with std band
ax1_lines = []
for (label, path), color in zip(files.items(), colors):
    by_bar = defaultdict(list)
    with open(path) as f:
        for row in csv.DictReader(f):
            by_bar[int(row['bar'])].append(float(row['equity']))
    
    bars = sorted(by_bar.keys())
    means = [np.mean(by_bar[b]) for b in bars]
    stds = [np.std(by_bar[b]) for b in bars]
    
    ln, = ax1.plot(bars, means, label=label, color=color, linewidth=2)
    ax1.fill_between(bars, 
                     [m - s for m, s in zip(means, stds)],
                     [m + s for m, s in zip(means, stds)],
                     alpha=0.15, color=color)
    ax1_lines.append(ln)

ax1.set_xlabel('Bar (cumulative)', fontsize=12)
ax1.set_ylabel('Portfolio Equity (× initial)', fontsize=12)
ax1.set_title('USDT Hedge Overlay — Equity Curves (mean ± std across 9 universes × 9 windows)', fontsize=14, fontweight='bold')
ax1.legend(loc='upper left', fontsize=10)
ax1.grid(True, alpha=0.3)
ax1.set_yscale('log')
ax1.set_ylim(0.8, None)

# Drawdown
ax2_lines = []
for (label, path), color in zip(files.items(), colors):
    by_bar = defaultdict(list)
    with open(path) as f:
        for row in csv.DictReader(f):
            by_bar[int(row['bar'])].append(float(row['equity']))
    
    bars = sorted(by_bar.keys())
    means = [np.mean(by_bar[b]) for b in bars]
    
    # Drawdown from running max
    running_max = np.maximum.accumulate(means)
    drawdown = [(m / rm - 1) * 100 for m, rm in zip(means, running_max)]
    
    ln, = ax2.plot(bars, drawdown, label=label, color=color, linewidth=2, alpha=0.8)
    ax2_lines.append(ln)

ax2.set_xlabel('Bar (cumulative)', fontsize=12)
ax2.set_ylabel('Drawdown (%)', fontsize=12)
ax2.set_title('Max Drawdown Over Time', fontsize=14, fontweight='bold')
ax2.legend(loc='lower left', fontsize=10)
ax2.grid(True, alpha=0.3)

plt.tight_layout()
plt.savefig('/home/ubuntu/.openclaw/workspace-krypto/charts/hedge_comparison_chart.png', dpi=150, bbox_inches='tight')
print("Saved hedge_comparison_chart.png")

# Print summary stats
for (label, path), color in zip(files.items(), colors):
    by_bar = defaultdict(list)
    with open(path) as f:
        for row in csv.DictReader(f):
            by_bar[int(row['bar'])].append(float(row['equity']))
    
    bars = sorted(by_bar.keys())
    means = [np.mean(by_bar[b]) for b in bars]
    running_max = np.maximum.accumulate(means)
    drawdown_pct = [(m / rm - 1) * 100 for m, rm in zip(means, running_max)]
    
    sharpe_proxy = means[-1] / (np.std(means) + 1e-9)
    final_eq = means[-1]
    max_dd = min(drawdown_pct)
    
    print(f'{label}: final={final_eq:.2f}x, Sharpe proxy={sharpe_proxy:.2f}, maxDD={max_dd:.1f}%')
