#!/usr/bin/env python3
"""Chart for cross-market audit."""
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import matplotlib.patches as mpatches
import numpy as np

assets  = ['SPY', 'GLD', 'QQQ', 'TLT', 'ILF', 'UUP', 'EWJ', 'FXE']
sharpes  = [0.87, 0.87, 0.76, 0.43, 0.25, 0.22, 0.12,-0.31]
cumrets  = [92,   57,   47,  -42,  -24,  -24,  -37,  -45]
ddvals   = [12.7,17.2, 20.1, 48.3, 49.9, 29.1, 45.9, 46.8]
descs    = ['US Equities','Gold','Nasdaq','Treasury','LatAm','USD','Japan','Euro']

fig, axes = plt.subplots(1, 2, figsize=(14, 5))
fig.patch.set_facecolor('#0d1117')

# Panel 1: Sharpe bars
ax1 = axes[0]
ax1.set_facecolor('#0d1117')
colors = ['#2ecc71' if s>0.5 else '#e74c3c' if s<0 else '#f39c12' for s in sharpes]
bars = ax1.barh(assets, sharpes, color=colors, edgecolor='none', height=0.6)
ax1.axvline(0.5, color='#f39c12', ls='--', lw=1.5, label='threshold (0.5)')
ax1.axvline(0.0, color='#555', ls='-', lw=0.8)
ax1.axvline(1.04, color='#3498db', ls=':', lw=2, label='Crypto equity 1.04')
ax1.set_xlabel('Daily Equity Sharpe', color='#ccc', fontsize=10)
ax1.set_title('Cross-Market Trend Following Audit\nTurtle+Chandelier (EP=21, Chand=28/2.0)', color='white', fontsize=11)
ax1.tick_params(colors='#aaa')
ax1.spines['bottom'].set_color('#333'); ax1.spines['left'].set_color('#333')
ax1.spines['top'].set_visible(False); ax1.spines['right'].set_visible(False)
ax1.set_xlim(-0.7, 1.3)
for bar, s in zip(bars, sharpes):
    ax1.text(s+0.03, bar.get_y()+bar.get_height()/2, f'{s:.2f}', va='center', ha='left', color='white', fontsize=9)
ax1.legend(facecolor='#1c1e26', labelcolor='#ccc', fontsize=8, loc='lower right')

# Panel 2: Scatter cumret vs sharpe
ax2 = axes[1]
ax2.set_facecolor('#0d1117')
for a, s, c in zip(assets, sharpes, cumrets):
    col = '#2ecc71' if s>0.5 else '#e74c3c' if s<0 else '#f39c12'
    ax2.scatter(s, c, s=120, color=col, zorder=5)
    ax2.annotate(f' {a}', (s, c), color='white', fontsize=9, va='center')
ax2.axhline(0, color='#555', lw=0.8)
ax2.axvline(0.5, color='#f39c12', ls='--', lw=1.5)
ax2.set_xlabel('Sharpe', color='#ccc', fontsize=10)
ax2.set_ylabel('Cumulative Return (%)', color='#ccc', fontsize=10)
ax2.set_title('Sharpe vs Return — Cross-Market', color='white', fontsize=11)
ax2.tick_params(colors='#aaa')
ax2.spines['bottom'].set_color('#333'); ax2.spines['left'].set_color('#333')
ax2.spines['top'].set_visible(False); ax2.spines['right'].set_visible(False)
ax2.set_xlim(-0.7, 1.3); ax2.set_ylim(-80, 130)

p1 = mpatches.Patch(color='#2ecc71', label='Pass (>0.5 Sharpe)')
p2 = mpatches.Patch(color='#e74c3c', label='Fail (<0 Sharpe)')
p3 = mpatches.Patch(color='#f39c12', label='Marginal')
ax2.legend(handles=[p1,p2,p3], facecolor='#1c1e26', labelcolor='#ccc', fontsize=8)

plt.tight_layout(pad=2)
plt.savefig('/home/ubuntu/.openclaw/workspace-krypto/charts/cross_market_audit.png', dpi=150, bbox_inches='tight', facecolor='#0d1117')
print("Saved charts/cross_market_audit.png")

# SPY per-year chart
years  = [2008,2009,2010,2011,2012,2013,2014,2015,2016,2017,2018,2019,2020,2021,2022,2023,2024,2025,2026]
bh_ret = [-36.2,22.7,13.1,0.9,14.2,29.0,14.6,1.3,13.6,20.8,-5.2,31.1,17.2,30.5,-18.6,26.7,26.7,18.0,1.9]
tu_ret = [-2.4, 1.9, 2.5, 1.1,-1.2, 1.5,-0.5,-0.6, 2.8, 1.6,-0.6, 1.3, 2.7, 0.1,-0.5, 0.2, 0.1, 0.9,-2.1]

fig2, ax = plt.subplots(figsize=(13, 4))
fig2.patch.set_facecolor('#0d1117')
ax.set_facecolor('#0d1117')
x = np.arange(len(years))
w = 0.35
ax.bar(x-w/2, bh_ret, w, label='SPY Buy-Hold', color='#555', alpha=0.7, edgecolor='none')
ax.bar(x+w/2, tu_ret, w, label='Turtle+Chandelier', color='#3498db', alpha=0.8, edgecolor='none')
ax.axhline(0, color='#888', lw=0.8)
ax.set_xticks(x)
yr_labels = [f'{y}' + ('*' if y in [2008,2020,2022] else '') for y in years]
ax.set_xticklabels(yr_labels, rotation=45, color='#aaa', fontsize=8)
ax.tick_params(colors='#aaa')
ax.set_ylabel('Annual Return (%)', color='#ccc', fontsize=9)
ax.set_title('SPY Annual Returns — Turtle+Chandelier vs Buy-Hold\n(* = crisis year)', color='white', fontsize=11)
ax.spines['bottom'].set_color('#333'); ax.spines['left'].set_color('#333')
ax.spines['top'].set_visible(False); ax.spines['right'].set_visible(False)
ax.legend(facecolor='#1c1e26', labelcolor='#ccc', fontsize=9)
plt.tight_layout()
plt.savefig('/home/ubuntu/.openclaw/workspace-krypto/charts/cross_market_spy_per_year.png', dpi=150, bbox_inches='tight', facecolor='#0d1117')
print("Saved charts/cross_market_spy_per_year.png")
