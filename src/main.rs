use binance_r_matrix::HistoricalDataBuilder;

fn main() {
    let data = HistoricalDataBuilder::default()
        .periods(7)
        .build()
        .unwrap();
    println!("{:?}", data);
}
