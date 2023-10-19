use binance::rest_model::KlineSummary;

#[inline(always)]
pub(crate) fn percentage_change(previous: f64, current: f64) -> f64 {
    (current - previous) / previous
}

#[inline(always)]
pub(crate) fn cr_ratio(candle: &KlineSummary) -> f64 {
    let max_body = candle.close.max(candle.open);
    let min_body = candle.close.min(candle.open);
    let body = max_body - min_body;
    let top_wick = candle.high - max_body;
    let bottom_wick = min_body - candle.low;
    let wick_sum = top_wick - bottom_wick;

    if body.abs() < f64::EPSILON {
        return wick_sum.signum()
    }
    (wick_sum / body).tanh()
}

#[cfg(test)]
pub mod tests {

    use super::*;

    #[test]
    fn test_percentage_change() {
        assert_eq!(percentage_change(10.0, 20.0), 1.0);
        assert_eq!(percentage_change(20.0, 10.0), -0.5);
        assert_eq!(percentage_change(10.0, 10.0), 0.0);
    }
}