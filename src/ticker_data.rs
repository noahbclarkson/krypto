use getset::Getters;

#[derive(Debug, Getters)]
pub struct TickerData {
    #[getset(get = "pub")]
    ticker: Box<str>,
}