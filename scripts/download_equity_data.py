#!/usr/bin/env python3
"""Download SPY/QQQ/GLD data from Yahoo Finance and save as parquet for cross-market walk-forward."""
import yfinance as yf
import polars as pl
import pandas as pd
import sys

TICKERS = ["SPY", "QQQ", "GLD"]
START = "2000-01-01"
END = "2026-04-16"
OUT_DIR = "data/cache"

for ticker in TICKERS:
    out_path = f"{OUT_DIR}/{ticker.lower()}_1d_equity.parquet"
    print(f"Downloading {ticker}...", end=" ", flush=True)
    try:
        df_yf: pd.DataFrame = yf.download(ticker, start=START, end=END, progress=False)
        if df_yf.empty:
            print(f"FAILED: empty download")
            continue
        
        # Flatten MultiIndex columns (yfinance 0.2.x+ uses Price/Ticker multi-index)
        if isinstance(df_yf.columns, pd.MultiIndex):
            df_yf.columns = df_yf.columns.get_level_values(0)
        
        # Build Polars with required column names (lowercase for Rust harness)
        # yfinance returns: Close, High, Low, Open, Volume (scalar columns)
        df = pl.DataFrame({
            "date": df_yf.index.strftime("%Y-%m-%d"),
            "close": df_yf["Close"].values.astype(float),
            "high": df_yf["High"].values.astype(float),
            "low": df_yf["Low"].values.astype(float),
            "volume": df_yf["Volume"].values.astype(float),
        })
        
        # Drop rows with nulls
        df = df.drop_nulls()
        
        # Write as parquet
        df.write_parquet(out_path)
        print(f"OK: {len(df)} rows, range {df['date'][0]} → {df['date'][-1]}")
        print(f"  close: {df['close'][0]:.2f} → {df['close'][-1]:.2f}")
    except Exception as e:
        print(f"FAILED: {e}")
        import traceback; traceback.print_exc()

print("\nAll done.")
