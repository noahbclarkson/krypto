sed -i '1i\
# Memory — Kira | 2026-04-07\
\
## 17:26 UTC — Evening Research Implementation & Diagnostics Cycle\
\
### What I did\
1. **DOGE Decomposition Audit:** Built and ran `doge_decomposition.rs` using the A/D Momentum strategy. Confirmed that across the Base5 universe + ADA, DOGE is a massive outlier: BTC +154.5%, ETH +728.8%, SOL +814.5%, DOGE +5013.6%. DOGE drives the outsized nominal returns of our multi-sleeve trend ensembles.\
2. **DVOL Options Skew Gate (Institutional Fear Filter):** Implemented and backtested `dvol_trend_gate.rs` using Deribit BTC DVOL history. Gating trend entries when DVOL spikes *kills* returns. Max options fear marks local bottoms (BUY signal), not a trend filter.\
3. **LOB Depth Imbalance Microstructure Proof of Concept:** Tested the Binance API v3 depth endpoint without auth. The NOBI edge is real but requires building a forward-looking live cache.\
\
### Project Mode Focus: Track C (Broaden edge discovery)\
Shifted focus away from parameter fishing (Track B) to Track C (factor allocation, order book collection).\
\
' /home/ubuntu/.openclaw/workspace-krypto/memory/2026-04-07.md
