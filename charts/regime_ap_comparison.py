import pandas as pd
import matplotlib.pyplot as plt
import os

candidates = [63, 37, 11, 7, 17, 12]
labels = {
    12: "AP=12 (Baseline)",
    63: "AP=63 (Winner)",
    37: "AP=37 (Runner-up 1)",
    11: "AP=11 (Runner-up 2)",
    7: "AP=7 (Runner-up 3)",
    17: "AP=17 (Runner-up 4)"
}

colors = {
    12: 'gray',
    63: 'blue',
    37: 'orange',
    11: 'green',
    7: 'red',
    17: 'purple'
}

plt.figure(figsize=(10, 6))

for ap in candidates:
    file_path = f"snapshots/ap_hyperopt/base5_agg_ap{ap}.csv"
    if os.path.exists(file_path):
        df = pd.read_csv(file_path)
        plt.plot(df['window'], df['agg_equity'], label=labels.get(ap, f"AP={ap}"), color=colors.get(ap, 'black'), marker='o', linewidth=2 if ap in [12, 63] else 1.5, linestyle='-' if ap==63 else '--' if ap==12 else ':')

plt.yscale('log')
plt.title("Base5 Aggregate Equity by REGIME_ATR_PERIOD (AP)")
plt.xlabel("Walk-Forward Window")
plt.ylabel("Compounded Equity (Log Scale)")
plt.grid(True, which="both", ls="--", alpha=0.5)
plt.legend()
plt.tight_layout()
plt.savefig("charts/comparison_chart.png", dpi=300)
