// Importing necessary dependencies
use crate::historical_data::Candlestick;

// Calculates and returns the percentage change between two values.
// start: Initial value
// end: Final value
pub fn change(start: f64, end: f64) -> f64 {
    if start == 0.0 {
        return 0.0; // Avoid division by zero
    }
    (end - start) / start * 100.0
}

// Computes the CR ratio of a candlestick.
// The CR ratio is the ratio of the difference of the top wick and bottom wick to the body of the candlestick.
// The tanh function is used to normalize the output.
// candle: A reference to the Candlestick struct
pub fn cr_ratio(candle: &Candlestick) -> f64 {
    let max_body = candle.close.max(candle.open);
    let min_body = candle.close.min(candle.open);
    let body = max_body - min_body;
    let top_wick = candle.high - max_body;
    let bottom_wick = min_body - candle.low;
    let wick_sum = top_wick - bottom_wick;

    if body.abs() < f64::EPSILON {
        return wick_sum.signum();
    } else {
        return (wick_sum / body).tanh();
    }
}

// Formats a floating-point number to a string representation with suffixes.
// The number is divided by 1000.0 and the appropriate suffix is appended until the number is less than 1000.
// num: The number to format
pub fn format_number(num: f64) -> String {
    let suffixes = [
        "", "K", "M", "B", "T", "Qa", "Qi", "Sx", "Sp", "Oc", "No", "De", "Ud", "Dd", "Td", "Qad",
        "Qid", "Sxd", "Spd", "Od", "Nd", "V", "Ct",
    ];

    let mut num = num;
    let mut index = 0;

    while num >= 1000.0 && index < suffixes.len() - 1 {
        num /= 1000.0;
        index += 1;
    }

    // Different formatting based on the magnitude of num.
    match num {
        n if n < 10.0 => format!("{:.2}{}", n, suffixes[index]),
        n if n < 100.0 => format!("{:.1}{}", n, suffixes[index]),
        _ => format!("{:.0}{}", num, suffixes[index]),
    }
}
