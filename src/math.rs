use ta::{Close, DataItem, High, Low, Open};

#[inline(always)]
pub fn percentage_change(previous: f32, current: f32) -> f32 {
    (current - previous) / previous * 100.0
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
        wick_sum.signum() as f32
    } else {
        (wick_sum / body).tanh() as f32
    }
}

pub fn format_number(num: f32) -> String {
    let original_num = num;
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

    match original_num {
        n if n < 10.0 => format!("{:.3}{}", num, suffixes[index]),
        n if n < 100.0 => format!("{:.2}{}", num, suffixes[index]),
        _ => format!("{:.1}{}", num, suffixes[index]),
    }
}

#[cfg(test)]
pub mod tests {

    use super::*;

    #[test]
    fn test_percentage_change() {
        assert_eq!(percentage_change(10.0, 20.0), 100.0);
        assert_eq!(percentage_change(20.0, 10.0), -50.0);
        assert_eq!(percentage_change(10.0, 10.0), 0.0);
    }

    #[test]
    fn test_cr_ratio() {
        let item = DataItem::builder()
            .open(10.0)
            .high(20.0)
            .low(5.0)
            .close(15.0)
            .volume(100.0)
            .build()
            .unwrap();

        assert_eq!(cr_ratio(&item), 0.0);

        let item = DataItem::builder()
            .open(10.0)
            .high(20.0)
            .low(5.0)
            .close(10.0)
            .volume(100.0)
            .build()
            .unwrap();

        assert_eq!(cr_ratio(&item), 1.0);

        let item = DataItem::builder()
            .open(10.0)
            .high(20.0)
            .low(5.0)
            .close(5.0)
            .volume(100.0)
            .build()
            .unwrap();

        assert_eq!(cr_ratio(&item), 0.9640276);
    }

    #[test]
    fn test_format_number() {
        assert_eq!(format_number(0.0), "0.000");
        assert_eq!(format_number(1.0), "1.000");
        assert_eq!(format_number(10.0), "10.00");
        assert_eq!(format_number(100.0), "100.0");
        assert_eq!(format_number(1_000.0), "1.0K");
        assert_eq!(format_number(10_000.0), "10.0K");
        assert_eq!(format_number(100_000.0), "100.0K");
        assert_eq!(format_number(1_000_000.0), "1.0M");
        assert_eq!(format_number(10_000_000.0), "10.0M");
        assert_eq!(format_number(100_000_000.0), "100.0M");
        assert_eq!(format_number(1_000_000_000.0), "1.0B");
        assert_eq!(format_number(10_000_000_000.0), "10.0B");
        assert_eq!(format_number(100_000_000_000.0), "100.0B");
        assert_eq!(format_number(1_000_000_000_000.0), "1.0T");
        assert_eq!(format_number(10_000_000_000_000.0), "10.0T");
        assert_eq!(format_number(100_000_000_000_000.0), "100.0T");
        assert_eq!(format_number(1_000_000_000_000_000.0), "1.0Qa");
        assert_eq!(format_number(10_000_000_000_000_000.0), "10.0Qa");
        assert_eq!(format_number(100_000_000_000_000_000.0), "100.0Qa");
    }
}
