use ta::{Close, DataItem, High, Low, Open};

const ZERO: &f32 = &0.0;

#[inline(always)]
pub fn change(start: &f32, end: &f32) -> f32 {
    if start == ZERO {
        return 0.0; // Avoid division by zero
    }
    (end - start) / start * 100.0
}

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

    match num {
        n if n < 10.0 => format!("{:.2}{}", n, suffixes[index]),
        n if n < 100.0 => format!("{:.1}{}", n, suffixes[index]),
        _ => format!("{:.0}{}", num, suffixes[index]),
    }
}

