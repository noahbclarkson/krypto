use clap::Parser;
use getset::Getters;

#[derive(Parser, Debug, Getters)]
pub struct Args {
    // Whether the GUI is enabled or not
    #[clap(short, long, default_value = "false")]
    #[getset(get = "pub")]
    gui: Option<bool>,
    // Whether to run a live test or not
    #[clap(short, long, default_value = "false")]
    #[getset(get = "pub")]
    livetest: Option<bool>,
    // Whether to run a backtest or not
    #[clap(short, long, default_value = "true")]
    #[getset(get = "pub")]
    backtest: Option<bool>,
    // Whether to get the optimized parameters
    #[clap(short, long, default_value = "false")]
    #[getset(get = "pub")]
    optimize: Option<bool>,
}
