# USER.md - About Noah

- **Name:** Noah
- **Pronouns:** he/him
- **Timezone:** Pacific/Auckland (Christchurch, NZ — UTC+13 standard, UTC+12 daylight)
- **Notes:** Very busy. Wants directness. Wants an autonomous agent that ships results without hand-holding.

## What He Wants

- A working crypto trading bot with the highest possible return and lowest possible risk
- Real, validated strategies — not backtested mirages
- Research, build, test, iterate independently
- Progress updates in Discord (#krypto channel) — **always include charts** for key changes, new strategies, and status updates
- Eventually: live paper trading + web frontend for monitoring

## Chart Standards (Noah's preference)

- **Include charts** in Discord updates for: key changes, new strategies, status reports
- Equity curves → **log scale** (critical for comparing strategies with very different return magnitudes)
- Drawdowns → **linear scale**
- Always include caption + key metrics (Sharpe, MaxDD, final equity) in the message
- Charts must come from the validated harness — never approximate or ad-hoc
- Generate:
  1. `cargo run --example progress_equity_curves` → `snapshots/progress_equity_curves.csv`
  2. `python3 -c "exec(open('charts/plot_progress.py').read())"` → PNGs in `charts/`
  3. `message(action=send, channel=discord, target=channel:1484783323497762816, media=<path>, message=<caption>)`

## Working Style

- Don't ask for permission on implementation choices — just try it and report results
- Be honest about failures — log them to GRAVEYARD.md
- Push freely to `v2-rewrite` branch, never to `main`
- Surface blockers early; don't spin on them silently for multiple sessions
