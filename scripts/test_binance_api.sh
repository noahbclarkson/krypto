#!/bin/bash
# Test Binance API connectivity and data format
# Run from krypto root: ./scripts/test_binance_api.sh

set -e

SYMBOL="${1:-BTCUSDT}"
INTERVAL="${2:-1h}"
LIMIT="${3:-10}"

echo "=== Binance API Test ==="
echo "Symbol: $SYMBOL"
echo "Interval: $INTERVAL"
echo "Limit: $LIMIT"
echo ""

# Test API connectivity
echo "1. Testing API connectivity..."
RESPONSE=$(curl -s -w "\n%{http_code}" "https://api.binance.com/api/v3/klines?symbol=$SYMBOL&interval=$INTERVAL&limit=$LIMIT")
HTTP_CODE=$(echo "$RESPONSE" | tail -n1)
BODY=$(echo "$RESPONSE" | sed '$d')

if [ "$HTTP_CODE" != "200" ]; then
    echo "❌ API request failed with HTTP $HTTP_CODE"
    echo "$BODY"
    exit 1
fi

echo "✅ API connected (HTTP $HTTP_CODE)"
echo ""

# Validate response format
echo "2. Validating response format..."
COUNT=$(echo "$BODY" | jq 'length')
if [ "$COUNT" != "$LIMIT" ]; then
    echo "⚠️  Expected $LIMIT candles, got $COUNT"
else
    echo "✅ Received $COUNT candles"
fi

# Check data structure
echo ""
echo "3. Sample candle structure:"
echo "$BODY" | jq '.[0]' | head -12

# Check for valid OHLCV data
echo ""
echo "4. Latest candle summary:"
echo "$BODY" | jq -r '.[-1] | "Time: \(.[] | . as $t | now * 1000 | if . < $t then "future" else (($t - .) / 3600000 | floor | tostring + "h ago") end) | Open: \(.[] | . as $t | .[1]) | High: \(.[] | . as $t | .[2]) | Low: \(.[] | . as $t | .[3]) | Close: \(.[] | . as $t | .[4]) | Volume: \(.[] | . as $t | .[5])"' 2>/dev/null || \
echo "$BODY" | jq '.[-1] | {open_time: .[0], open: .[1], high: .[2], low: .[3], close: .[4], volume: .[5]}'

echo ""
echo "=== Test Complete ==="
