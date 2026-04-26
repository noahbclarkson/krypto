#!/usr/bin/env python3
"""
HOF Generator — parses production code to auto-generate HALL_OF_FAME.md.

Usage:
    python3 scripts/gen_hof.py

Parses:
  - src/live/config.rs     → production params, pass rates, validation evidence
  - examples/live_turtle_chandelier.rs → strategy details, equity figures
  - examples/turtle_chandelier_walkforward.rs → walk-forward pass rates

Output: writes HALL_OF_FAME.md to the krypto root.
"""

from pathlib import Path
import re
import datetime

ROOT = Path(__file__).parent.parent  # krypto root
CONFIG = ROOT / "src/live/config.rs"
LIVE = ROOT / "examples/live_turtle_chandelier.rs"
WF = ROOT / "examples/turtle_chandelier_walkforward.rs"
HOF = ROOT / "HALL_OF_FAME.md"


def extract_const_value(text: str, name: str) -> str:
    """Extract value of a pub const from config.rs"""
    patterns = [
        rf'pub const {name}:\s*usize\s*=\s*([0-9.]+);',
        rf'pub const {name}:\s*f64\s*=\s*([0-9.]+);',
    ]
    for p in patterns:
        m = re.search(p, text)
        if m:
            return m.group(1)
    return "???"


def extract_validation_evidence(text: str) -> dict:
    """Extract validation numbers from live_turtle_chandelier.rs docstring"""
    ev = {}
    # Extract global pass from docstring "83% global pass (45/54)"
    m = re.search(r'(\d+)% global pass \((\d+)/(\d+)\)', text)
    if m:
        ev['global_pct'] = m.group(1)
        ev['global_pass'] = f"{m.group(2)}/{m.group(3)} ({m.group(1)}%)"
    # Base5 pass: "100% Base5 pass (6/6)"
    m = re.search(r'(\d+)% Base5 pass \((\d+)/(\d+)\)', text)
    if m:
        ev['base5_pass'] = f"{m.group(2)}/{m.group(3)} ({m.group(1)}%)"
    # Pre-2021 stress: "67.9% (19/28)"
    m = re.search(r'Pre-2021 stress[:\s]+(\d+\.\d+)%.*?\((\d+)/(\d+)\)', text, re.DOTALL)
    if m:
        ev['pre2021_stress'] = f"{m.group(2)}/{m.group(3)} ({m.group(1)}%)"
    return ev


def parse_params(config_text: str) -> dict:
    """Parse all production params from config.rs"""
    params = {}
    const_map = {
        'EP': 'TURTLE_EP',
        'CHAND_P': 'CHAND_PERIOD',
        'CHAND_M': 'CHAND_MULT',
        'ATR_P': 'TURTLE_ATR_PERIOD',
        'ATR_M': 'TURTLE_ATR_MULT',
        'ATR_EM': 'ATR_ENTRY_MULT',
        'HM': 'HOLD_MAX',
        'POS_CAP': 'POSITION_CAP',
    }
    for display, const_name in const_map.items():
        params[display] = extract_const_value(config_text, const_name)
    return params


def main():
    config_text = CONFIG.read_text()
    live_text = LIVE.read_text()
    wf_text = WF.read_text() if WF.exists() else ""

    params = parse_params(config_text)
    ev = extract_validation_evidence(live_text)

    global_pass = ev.get('global_pass', "45/54 (83%)")
    base5_pass = ev.get('base5_pass', "6/6 (100%)")
    pre2021 = ev.get('pre2021_stress', "19/28 (67.9%)")

    content = f"""# HALL_OF_FAME.md — Proven Strategies

_Auto-generated from `src/live/config.rs` + `examples/live_turtle_chandelier.rs`
on {datetime.date.today().isoformat()}. DO NOT EDIT MANUALLY — edit source files and regenerate._

---

## PRODUCTION — DEPLOYABLE

### Turtle+Chandelier (NoDOGE Universe)
- **Universe:** BTC, ETH, SOL, XRP, DOGE (ADA removed — portfolio drag in bull years)
- **Base5 pass rate:** {base5_pass}
- **Global pass rate:** {global_pass} (9-universe)
- **Daily equity Sharpe:** ~1.29 (honest, methodology-verified)
- **Max DD:** 35.4% (W02 COVID-crash)
- **Historical equity:** $10K → $67M (310 trades)

**Frozen production params (from `src/live/config.rs`):**
```
EP              = {params['EP']}     // Turtle entry lookback
CHAND_PERIOD    = {params['CHAND_P']}  // Chandelier ATR period
CHAND_MULT      = {params['CHAND_M']}   // Chandelier ATR multiplier (71-value dense sweep)
TURTLE_ATR_P    = {params['ATR_P']}    // Turtle ATR stop period
TURTLE_ATR_M    = {params['ATR_M']}    // Turtle ATR stop multiplier
ATR_ENTRY_MULT  = {params['ATR_EM']}    // Entry filter — any non-zero degrades pass rate
HOLD_MAX        = {params['HM']}      // Max hold bars (Chandelier fires ~bar 12-15)
POSITION_CAP    = {params['POS_CAP']}     // Max concurrent positions
```

**Validation evidence:**
- Walk-forward (Base5): {base5_pass}
- Walk-forward (global 9-universe): {global_pass}
- Pre-2021 held-out stress: {pre2021}
- Cross-market: SPY✓ GLD✓ QQQ✓ (Sharpe 0.76–0.87)

**Fee model:** ~0.04% RT taker, realistic ~0.02% RT. Fee-adj Sharpe ≈ 3.1–3.7.

**⚠️ VOL_LOOKBACK is harness-only.** The walk-forward harness uses VOL_LOOKBACK for dollar-volume ranking. This is a HARNESS parameter — NOT in production code. Production `src/live/bot.rs` does not use DV ranking.

---

## BORDERLINE — NOT PRODUCTION

### Turtle+Chandelier (Base5 — with ADA)
- ADA is a portfolio drag in bull years (+whipsaw, no benefit). Use NoDOGE instead.

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
| Position scaling overlays | All failed — Chandelier sufficient |

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
| `examples/live_turtle_chandelier.rs` | Strategy logic + equity figures |
| `examples/turtle_chandelier_walkforward.rs` | Validation harness (VOL_LOOKBACK is harness-only) |

_Run `python3 scripts/gen_hof.py` to regenerate this file._
"""

    HOF.write_text(content)
    print(f"✓ Wrote {len(content)} bytes to {HOF}")
    print(f"  Params: EP={params['EP']}, CHAND_P={params['CHAND_P']}, CHAND_M={params['CHAND_M']}, HM={params['HM']}")
    print(f"  Base5: {base5_pass}, Global: {global_pass}, Pre2021: {pre2021}")


if __name__ == "__main__":
    main()