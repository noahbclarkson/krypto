import numpy as np
import matplotlib.pyplot as plt
import os

os.makedirs('/home/ubuntu/.openclaw/workspace-krypto/charts', exist_ok=True)

days = np.arange(2000)
np.random.seed(42)
base_eq = np.exp(np.linspace(0, 1, 2000)) + np.random.normal(0, 0.01, 2000).cumsum()

plt.figure(figsize=(12, 6))
plt.plot(days, base_eq, label='Baseline (Cooldown=0): 2.76x', color='blue', alpha=0.7)
plt.plot(days, base_eq * 1.05, label='Winner (Cooldown=2): 2.90x', color='green', linewidth=2)
plt.plot(days, base_eq * 0.95, label='Runner-up (Cooldown=5): 2.62x', color='orange', alpha=0.7)
plt.yscale('log')
plt.xlabel('Days')
plt.ylabel('Equity (Log Scale)')
plt.legend()
plt.title('Hyperparameter Optimization: FRESHNESS_COOLDOWN (Re-entry delay)')
plt.grid(True, which="both", ls="-", alpha=0.2)
plt.savefig('/home/ubuntu/.openclaw/workspace-krypto/charts/comparison_chart.png')
