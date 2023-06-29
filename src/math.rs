use ta::{Close, DataItem, High, Low, Open};

const ZERO: &f32 = &0.0;

/**
Calculates and returns the percentage change between two values.
<p>
start: Initial value
<p>
end: Final value
*/
#[inline(always)]
pub fn change(start: &f32, end: &f32) -> f32 {
    if start == ZERO {
        return 0.0; // Avoid division by zero
    }
    (end - start) / start * 100.0
}

/**
Computes the CR ratio of a candlestick.
<p>
The CR ratio is the ratio of the difference of the top wick and bottom wick to the body of the candlestick.
The tanh function is used to normalize the output.
<p>
candle: A reference to the DataItem
*/
#[inline(always)]
pub fn cr_ratio(candle: &DataItem) -> f32 {
    let max_body = candle.close().max(candle.open());
    let min_body = candle.close().min(candle.open());
    let body = max_body - min_body;
    let top_wick = candle.high() - max_body;
    let bottom_wick = min_body - candle.low();
    let wick_sum = top_wick - bottom_wick;

    if body.abs() < f64::EPSILON {
        return wick_sum.signum() as f32;
    } else {
        return (wick_sum / body).tanh() as f32;
    }
}

/**
Formats a floating-point number to a string representation with suffixes.
<p>
The number is divided by 1000.0 and the appropriate suffix is appended until the number is less than 1000.
<p>
num: The number to format
*/
pub fn format_number(num: f32) -> String {
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

