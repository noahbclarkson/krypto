import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import matplotlib.ticker as mticker
import pandas as pd
import numpy as np

df = pd.read_csv('snapshots/live_bot_exact_equity.csv', header=0)
print("Columns:", df.columns.tolist())
print("First row:", df.iloc[0].tolist())
print("Last row:", df.iloc[-1].tolist())
print("Rows:", len(df))
