import re
import pandas as pd
import matplotlib.pyplot as plt

# Read from log.txt
logs = ""
with open("log.txt", "r") as f:
    logs = f.read()

# -------- Parse the log for n, d, Sharpe --------
pattern = re.compile(r'n:\s*(\d+),\s*depth:\s*(\d+).*?Sharpe:\s*([-\d\.]+)')
pairs = re.findall(pattern, logs)

data = {}
for n, d, s in pairs:
    n, d, s = int(n), int(d), float(s)
    data.setdefault((n, d), []).append(s)

avg_sharpe = {k: sum(v)/len(v) for k, v in data.items()}

# Build a DataFrame covering all n & d seen (0 for missing)
n_vals = sorted({k[0] for k in avg_sharpe})
d_vals = sorted({k[1] for k in avg_sharpe})
df = pd.DataFrame(0.0, index=n_vals, columns=d_vals)

for (n, d), val in avg_sharpe.items():
    df.loc[n, d] = val

# Try to show the DataFrame nicely if ace_tools is available
try:
    import ace_tools
    ace_tools.display_dataframe_to_user("Average Sharpe by (n,d)", df)
except ImportError:
    print("Average Sharpe by (n,d):")
    print(df)

# -------- Plot the heat‑map --------
fig, ax = plt.subplots()
cax = ax.imshow(df.values, cmap='viridis', aspect='auto')
ax.set_xticks(range(len(d_vals)))
ax.set_xticklabels(d_vals)
ax.set_yticks(range(len(n_vals)))
ax.set_yticklabels(n_vals)
ax.set_xlabel("d (depth)")
ax.set_ylabel("n")
ax.set_title("Average Sharpe Ratio by n and d")
fig.colorbar(cax, ax=ax)
plt.tight_layout()
plt.show()
