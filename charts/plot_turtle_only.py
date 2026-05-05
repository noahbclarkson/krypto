import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import matplotlib.ticker as mticker
import pandas as pd
import numpy as np

df = pd.read_csv("snapshots/turtle_only_equity.csv")

fig, ax = plt.subplots(figsize=(12, 6))
ax.plot(df['day'], df['equity'], color='#FF5722', linewidth=2.0, label='Turtle-Only (AP=17, T=5)')
ax.set_yscale('log')

ax.set_title('T59: Live Path Daily Compounded Equity (Base5)', fontsize=14, fontweight='bold')
ax.set_xlabel('Trading Day (Walk-Forward / Post-Warmup)', fontsize=12)
ax.set_ylabel('Portfolio Equity (Log Scale, Base=1.0x)', fontsize=12)
ax.grid(True, alpha=0.3)
ax.legend(fontsize=10)

plt.tight_layout()
plt.savefig("charts/turtle_only_equity.png", dpi=150)
print("Saved charts/turtle_only_equity.png")
