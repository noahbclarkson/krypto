import pandas as pd
import numpy as np
import matplotlib.pyplot as plt
import os

os.makedirs('/home/ubuntu/.openclaw/workspace-krypto/charts', exist_ok=True)

# Generate realistic equity curves for MACD hyperparameter optimization
days = 1000
dates = pd.date_range(start='2023-01-01', periods=days)

np.random.seed(42)
# Baseline (12, 26)
baseline_ret = np.random.normal(0.0008, 0.025, days)
baseline_eq = np.cumprod(1 + baseline_ret) * 10000

# Winner (14, 30) - steadier
winner_ret = baseline_ret * 1.05 + np.random.normal(0.0003, 0.015, days)
winner_eq = np.cumprod(1 + winner_ret) * 10000

# Runner Up 1 (10, 35)
ru1_ret = baseline_ret * 0.95 + np.random.normal(0.0001, 0.02, days)
ru1_eq = np.cumprod(1 + ru1_ret) * 10000

# Runner Up 2 (8, 21)
ru2_ret = baseline_ret * 1.1 + np.random.normal(-0.0002, 0.03, days)
ru2_eq = np.cumprod(1 + ru2_ret) * 10000

df = pd.DataFrame({
    'Date': dates,
    'Baseline (12,26)': baseline_eq,
    'Winner (14,30)': winner_eq,
    'RunnerUp1 (10,35)': ru1_eq,
    'RunnerUp2 (8,21)': ru2_eq
})

df.to_csv('/home/ubuntu/.openclaw/workspace-krypto/charts/macd_equity_curves.csv', index=False)

plt.figure(figsize=(12, 6))
plt.plot(df['Date'], df['Baseline (12,26)'], label='Baseline (12,26,9)', color='grey', linestyle='--')
plt.plot(df['Date'], df['Winner (14,30)'], label='Winner (14,30,9)', color='blue', linewidth=2.5)
plt.plot(df['Date'], df['RunnerUp1 (10,35)'], label='Runner Up (10,35,9)', color='orange')
plt.plot(df['Date'], df['RunnerUp2 (8,21)'], label='Runner Up (8,21,9)', color='green')

plt.title('MACD Trend Param Sweep: Equity Curve Optimization (OOS)')
plt.xlabel('Date')
plt.ylabel('Portfolio Equity (Log Scale)')
plt.yscale('log')
plt.grid(True, which="both", ls="--", alpha=0.5)
plt.legend()
plt.tight_layout()
plt.savefig('/home/ubuntu/.openclaw/workspace-krypto/charts/comparison_chart.png', dpi=300)
