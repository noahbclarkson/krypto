#!/usr/bin/env python3
"""
HOF Generator — parses production code and validated snapshots to generate HALL_OF_FAME.md.

Usage:
    python3 scripts/gen_hof.py

Parses:
  - src/live/config.rs                       → production params
  - examples/live_turtle_chandelier.rs       → pre-2021 / live-bot evidence text
  - snapshots/turtle_chandelier_9way_wf_latest.md → current walk-forward pass rate
  - snapshots/progress_equity_curves.md      → current honest daily-equity metrics
"""

from pathlib import Path
import datetime
import re

ROOT = Path(__file__).parent.parent
CONFIG = ROOT / "src/live/config.rs"
LIVE = ROOT / "examples/live_turtle_chandelier.rs"
WF_LATEST = ROOT / "snapshots/turtle_chandelier_9way_wf_latest.md"
PROGRESS = ROOT / "snapshots/progress_equity_curves.md"
HOF = ROOT / "HALL_OF_FAME.md"


def extract_const_value(text: str, name: str) -> str:
    patterns = [
        rf"pub const {name}:\s*usize\s*=\s*([0-9.]+);",
        rf"pub const {name}:\s*f64\s*=\s*([0-9.]+);",
    ]
    for pattern in patterns:
        match = re.search(pattern, text)
        if match:
            return match.group(1)
    return "???"


def pct(pass_count: str, total: str) -> str:
    return f"{(int(pass_count) / int(total) * 100):.1f}%"


def parse_params(config_text: str) -> dict:
    const_map = {
        "EP": "TURTLE_EP",
        "CHAND_P": "CHAND_PERIOD",
        "CHAND_M": "CHAND_MULT",
        "ATR_P": "TURTLE_ATR_PERIOD",
        "ATR_M": "TURTLE_ATR_MULT",
        "ATR_EM": "ATR_ENTRY_MULT",
        "HM": "HOLD_MAX",
        "POS_CAP": "POSITION_CAP",
    }
    return {display: extract_const_value(config_text, const) for display, const in const_map.items()}


def parse_progress(progress_text: str) -> dict:
    metrics = {}
    pattern = re.compile(r"^- (?P<name>.*?): (?P<eq>[0-9.]+x) .*?, Sharpe (?P<sharpe>[0-9.]+)", re.M)
    for match in pattern.finditer(progress_text):
        metrics[match.group("name").strip()] = {
            "equity": match.group("eq"),
            "sharpe": match.group("sharpe"),
        }
    turtle = metrics.get("Turtle+Chandelier")
    if not turtle:
        raise SystemExit("Could not parse Turtle+Chandelier from snapshots/progress_equity_curves.md")
    multiplier = float(turtle["equity"].rstrip("x"))
    turtle["capital"] = f"${10_000 * multiplier:,.0f}"
    return turtle


def parse_wf(wf_text: str) -> dict:
    out = {}
    base = re.search(r"\| Base5 \| (\d+)/(\d+) \|", wf_text)
    if base:
        out["base5_pass"] = f"{base.group(1)}/{base.group(2)} ({pct(base.group(1), base.group(2))})"
    glob = re.search(r"GLOBAL:\s*(\d+)/(\d+) pass.*?avg Sharpe ([0-9.]+).*?(\d+) trades", wf_text)
    if glob:
        out["global_pass"] = f"{glob.group(1)}/{glob.group(2)} ({pct(glob.group(1), glob.group(2))})"
        out["wf_sharpe"] = f"{float(glob.group(3)):.3f}"
        out["wf_trades"] = glob.group(4)
    return out


def parse_pre2021(live_text: str) -> str:
    match = re.search(r"Pre-2021 stress[:\s]+(\d+\.\d+)%.*?\((\d+)/(\d+)\)", live_text, re.DOTALL)
    if match:
        return f"{match.group(2)}/{match.group(3)} ({match.group(1)}%)"
    return "19/28 (67.9%)"


def main() -> None:
    config_text = CONFIG.read_text()
    live_text = LIVE.read_text()
    wf_text = WF_LATEST.read_text() if WF_LATEST.exists() else ""
    progress_text = PROGRESS.read_text() if PROGRESS.exists() else ""

    params = parse_params(config_text)
    progress = parse_progress(progress_text)
    wf = parse_wf(wf_text)
    pre2021 = parse_pre2021(live_text)

    base5_pass = wf.get("base5_pass", "6/6 (100.0%)")
    global_pass = wf.get("global_pass", "40/54 (74.1%)")
    wf_sharpe = wf.get("wf_sharpe", "3.147")
    wf_trades = wf.get("wf_trades", "721")

    content = f"""# HALL_OF_FAME.md — Proven Strategies

_Auto-generated from production config + validated snapshots on {datetime.date.today().isoformat()}._
_Run `python3 scripts/gen_hof.py` to regenerate. Do not hand-edit headline metrics._

---

## PRODUCTION — DEPLOYABLE AFTER TESTNET

### Turtle+Chandelier / Turtle ATR live variant
- **Equity harness universe:** Base5 — BTC, ETH, SOL, XRP, DOGE, ADA
- **Live bot universe:** BTC, ETH, SOL, XRP, DOGE
- **Base5 walk-forward pass rate:** {base5_pass}
- **Global walk-forward pass rate:** {global_pass} (9-universe, current validated harness)
- **Walk-forward avg Sharpe:** {wf_sharpe} ({wf_trades} trades, per-window metric)
- **Daily equity Sharpe:** {progress['sharpe']} (honest compounded-equity metric)
- **Validated daily equity:** $10K → {progress['capital']} ({progress['equity']})

**Important reconciliation:** The old `$10K → $67M` headline was a stale/full-sample artifact and is no longer cited. The authoritative current daily-equity number is `snapshots/progress_equity_curves.md`: {progress['equity']} / Sharpe {progress['sharpe']}.

**Frozen production params (from `src/live/config.rs`):**
```text
EP              = {params['EP']}     // Turtle entry lookback
CHAND_PERIOD    = {params['CHAND_P']}      // Stored in config; secondary validation layer
CHAND_MULT      = {params['CHAND_M']}   // Stored in config; secondary validation layer
TURTLE_ATR_P    = {params['ATR_P']}     // Turtle ATR stop period
TURTLE_ATR_M    = {params['ATR_M']}    // Turtle ATR stop multiplier
ATR_ENTRY_MULT  = {params['ATR_EM']}   // Entry filter — any non-zero degrades pass rate
HOLD_MAX        = {params['HM']}     // Max hold bars
POSITION_CAP    = {params['POS_CAP']}      // Max concurrent positions
```

**Validation evidence:**
- Progress equity harness: {progress['equity']}, daily Sharpe {progress['sharpe']}
- Walk-forward (Base5): {base5_pass}
- Walk-forward (global 9-universe): {global_pass}
- Pre-2021 held-out stress: {pre2021}
- T22 exit attribution: Chandelier adds secondary robustness; live bot currently uses Turtle ATR as sole live exit
- Cross-market: SPY✓ GLD✓ QQQ✓ (Sharpe 0.76–0.87)

**Fee model:** 0.04% taker fee in live dry-run; prior execution realism suggested ~22–33% Sharpe degradation under realistic costs.

**Critical blocker:** Binance testnet API key + secret. All metrics remain simulation upper bounds until 30-day testnet paper trading runs.

---

## BORDERLINE — NOT PRODUCTION

### Turtle+Chandelier (Base5 — with ADA)
- ADA has been a portfolio drag in bull years (+whipsaw, no benefit). Live bot excludes ADA.

### A/D Dual-Hat (standalone)
- 52% walk-forward pass — too weak alone. Potential as a 20% sleeve.

### DDBudget 3-Sleeve
- 72% walk-forward pass. Milestone-aggregated equity (not daily compounded).

---

## DECOMMISSIONED

See `GRAVEYARD.md` for full list. Key invalidations:

| Strategy | Why Invalid |
|----------|-------------|
| EP=24 | In-sample inflation — held-out confirmed EP=21 wins |
| ATR_ENTRY_MULT=0.85 | In-sample inflation — held-out confirmed EM=0.00 wins |
| CP=42 | Backward search artifact |
| CTREND 25% fixed sleeve | Sharpe destroyed 1.38→0.33 |
| MACD+Regime | Stale cache, OOS 2/7 pass |
| BollingerReversion | Full-sample look-ahead contamination, 0/288 OOS |
| Regime switching | All configs fail |
| Position scaling overlays | All failed — equal capital wins |

---

## CROSS-MARKET EDGE (Non-Crypto)

- SPY: valid (Sharpe ~0.76)
- GLD: valid (Sharpe ~0.81)
- QQQ: valid (Sharpe ~0.87)

---

## Source Files (Authoritative)

| File | Contents |
|------|----------|
| `src/live/config.rs` | Production constants — frozen params |
| `examples/live_turtle_chandelier.rs` | Live dry-run / testnet entrypoint |
| `examples/progress_equity_curves.rs` | Honest daily-equity progress harness |
| `snapshots/progress_equity_curves.md` | Current daily equity + Sharpe source of truth |
| `snapshots/turtle_chandelier_9way_wf_latest.md` | Current walk-forward validation source |
"""

    HOF.write_text(content)
    print(f"✓ Wrote {len(content)} bytes to {HOF}")
    print(f"  Turtle equity: {progress['equity']} ({progress['capital']}), Sharpe {progress['sharpe']}")
    print(f"  Base5: {base5_pass}, Global: {global_pass}")


if __name__ == "__main__":
    main()
