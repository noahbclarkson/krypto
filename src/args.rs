use clap::Parser;
use getset::Getters;

#[derive(Parser, Debug, Getters)]
pub struct Args {
    #[clap(short, long, default_value = "false")]
    #[getset(get = "pub")]
    livetest: Option<bool>,
    #[clap(short, long, default_value = "true")]
    #[getset(get = "pub")]
    backtest: Option<bool>,
    #[clap(short, long, default_value = "false")]
    #[getset(get = "pub")]
    optimize: Option<bool>,
    #[clap(short, long, default_value = "false")]
    #[getset(get = "pub")]
    run: Option<bool>,
}
