"""Plot Live-Compatible Walk-Forward results from snapshots/live_compatible_*_equity.csv"""
import os

OUTDIR = "/home/ubuntu/.openclaw/workspace-krypto/krypto/charts"
os.makedirs(OUTDIR, exist_ok=True)

universes = [
    ("Base5",         4, 2.35, 157.7, 38.2, 81),
    ("NoDOGE",        5, 4.41,  56.1, 31.7, 80),
    ("Legacy4",       5, 4.18, 135.6, 28.0, 77),
    ("Legacy5BNB",    6, 3.00, 137.7, 30.4, 71),
    ("OldGuardNoBNB", 5, 2.37, 107.2, 34.3, 81),
    ("LargeCaps5",    5, 4.52,  51.5, 31.1, 78),
    ("Legacy3",       6, 4.87,  99.3, 28.6, 79),
    ("LowVolume5",    4, 1.70, 285.9, 42.3, 90),
    ("OldGuard4",     5, 2.42,  57.8, 35.4, 86),
]

def load_equity(universe):
    path = f"/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots/live_compatible_{universe}_equity.csv"
    rows = []
    with open(path) as f:
        next(f)
        for line in f:
            w, eq = line.strip().split(",")
            rows.append((int(w), float(eq)))
    return rows

try:
    all_equities = {u: load_equity(u) for u, *_ in universes}
    base5_agg = 1.0
    for _, e in all_equities["Base5"]:
        base5_agg *= e
    have_data = True
except Exception as ex:
    print(f"Data load error: {ex}")
    have_data = False

import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import matplotlib.gridspec as gridspec

fig = plt.figure(figsize=(14, 10))
fig.suptitle("Live-Compatible Walk-Forward — Turtle-Only Exit\n(src/live/bot.rs post 2026-05-01 bug fix, ATR_RANK=5)",
            fontsize=13, fontweight='bold')

gs = gridspec.GridSpec(2, 2, figure=fig, hspace=0.38, wspace=0.32)

# Panel 1: Base5 per-window equity (bar)
ax1 = fig.add_subplot(gs[0, 0])
if have_data:
    wins = all_equities["Base5"]
    ax1.bar([w for w, e in wins], [e for _, e in wins],
            color=['#27ae60' if e >= 1.0 else '#c0392b' for _, e in wins],
            edgecolor='black', linewidth=0.5)
    ax1.axhline(1.0, color='black', lw=1, ls='--')
    ax1.set_yscale('log')
    ax1.set_ylim(bottom=0.01)
    ax1.set_title("Base5 Per-Window Equity")
    ax1.set_xlabel("Window"); ax1.set_ylabel("Equity (x, log scale)")
    ax1.grid(True, alpha=0.3)
    for i, (w, e) in enumerate(wins):
        ax1.text(i, e * 1.15, f'{e:.2f}x', ha='center', va='bottom', fontsize=7.5)

# Panel 2: Base5 cumulative equity across windows
ax2 = fig.add_subplot(gs[0, 1])
if have_data:
    cum = [1.0]
    for _, e in all_equities["Base5"]:
        cum.append(cum[-1] * e)
    ax2.plot(range(len(cum)), cum, 'b-o', linewidth=2, markersize=5)
    ax2.set_yscale('log')
    ax2.set_ylim(bottom=0.01)
    ax2.set_title(f"Base5 Cumulative Equity — {cum[-1]:.2f}x final ({len(cum)-1} windows)")
    ax2.set_xlabel("Window"); ax2.set_ylabel("Equity (x, log scale)")
    ax2.axhline(1.0, color='black', lw=1, ls='--')
    ax2.grid(True, alpha=0.3)
    for i, c in enumerate(cum):
        ax2.annotate(f'{c:.2f}x', (i, c), textcoords='offset points', xytext=(5, 5), fontsize=7)

# Panel 3: Per-universe pass rate
ax3 = fig.add_subplot(gs[1, 0])
u_names = [u for u, *_ in universes]
passes = [p for _, p, *_ in universes]
total = 7
bars = ax3.bar(u_names, [p/total*100 for p in passes],
               color=['#27ae60' if p == total else '#e67e22' if p/total >= 0.57 else '#c0392b'
                      for p in passes],
               edgecolor='black', linewidth=0.5)
ax3.axhline(70, color='orange', lw=1.5, ls='--', label='70% threshold')
ax3.set_title("Pass Rate by Universe (7 windows each)")
ax3.set_ylabel("Pass Rate (%)"); ax3.set_ylim(0, 108)
ax3.tick_params(axis='x', rotation=30)
ax3.grid(True, alpha=0.3, axis='y')
for bar, p in zip(bars, passes):
    ax3.text(bar.get_x() + bar.get_width()/2, bar.get_height() + 1,
             f'{p}/7', ha='center', va='bottom', fontsize=9)

# Panel 4: Metrics table
ax4 = fig.add_subplot(gs[1, 1])
ax4.axis('off')
gp = sum(p for _, p, *_ in universes)
gt = total * len(universes)
gsh = sum(s for _, _, s, *_ in universes) / len(universes)
grt = sum(r for _, _, _, r, *_ in universes) / len(universes)
gdd = sum(d for _, _, _, _, d, *_ in universes) / len(universes)
gtr = sum(t for _, _, _, _, _, t in universes)

caption = (
    "LIVE_COMPATIBLE | Turtle-only exit (src/live/bot.rs after 2026-05-01 fix)\n"
    "Entry: Turtle(EP=21) + ATR_RANK(AP=12,LB=42,T=5) | Exit: Turtle ATR(24,M=2.0) + HOLD_MAX=12\n"
    "Risk overlay: USDT 30% size when BTC 21d ATR > 75th pct 252d | Fee: 0.10% taker/side\n"
    f"Result: {gp}/{gt} ({gp/gt*100:.1f}% global) | Base5 {passes[0]}/7 | Sharpe {gsh:.2f}"
)
ax4.set_title(caption, fontsize=9, pad=8, loc='center')

table_data = [
    ["Metric", "Global", "Base5"],
    ["Pass Rate", f"{gp}/{gt} ({gp/gt*100:.1f}%)", f"{passes[0]}/7 ({passes[0]/7*100:.0f}%)"],
    ["Avg Sharpe", f"{gsh:.2f}", f"{universes[0][2]:.2f}"],
    ["Avg Return", f"{grt:.1f}%", f"{universes[0][3]:.1f}%"],
    ["Avg DD", f"{gdd:.1f}%", f"{universes[0][4]:.1f}%"],
    ["Total Trades", str(gtr), str(universes[0][5])],
    ["Agg Equity", "—", f"{base5_agg:.2f}x" if have_data else "N/A"],
]

tbl = ax4.table(cellText=table_data[1:], colLabels=table_data[0],
               cellLoc='center', loc='center', bbox=[0.05, 0.15, 0.9, 0.70])
tbl.auto_set_font_size(False); tbl.set_fontsize(9.5); tbl.scale(1, 1.8)
for (row, col), cell in tbl.get_celld().items():
    if row == 0:
        cell.set_facecolor('#2c3e50'); cell.set_text_props(color='white', fontweight='bold')
    elif col == 0:
        cell.set_facecolor('#ecf0f1'); cell.set_text_props(fontweight='bold')
    else:
        cell.set_facecolor('#ffffff')

OUT = f"{OUTDIR}/live_compatible_wf.png"
plt.savefig(OUT, dpi=180, bbox_inches='tight')
print(f"Saved {OUT}")
