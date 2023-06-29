use binance::{api::Binance, margin::Margin, rest_model::MarginAccountDetails};
use getset::Getters;

#[derive(Getters)]
#[getset(get = "pub")]
pub struct KryptoAccount {
    account: Margin,
}

impl KryptoAccount {
    pub fn new(config: &crate::config::Config) -> Self {
        let binance_config = binance::config::Config::default();
        let account = Binance::new_with_config(
            config.api_key().clone(),
            config.api_secret().clone(),
            &binance_config);
        Self {
            account,
        }
    }

    pub async fn details(&self) -> Result<MarginAccountDetails, Box<dyn std::error::Error>> {
        let details = self.account.details().await?;
        Ok(details)
    }

    pub async fn get_balance(&self, asset_string: &str) -> Result<f64, Box<dyn std::error::Error>> {
        let details = self.details().await?;
        let asset_string = parse_asset_string(asset_string);
        let asset = details.user_assets.iter().find(|x| x.asset == asset_string);
        match asset {
            Some(asset) => Ok(asset.free),
            None => Err(Box::new(AssetNotFoundError {
                asset: asset_string,
            })),
        }
    }
}

fn parse_asset_string(asset_string: &str) -> String {
    let asset_string = asset_string.trim().to_uppercase().to_string();
    asset_string
}

#[derive(Debug)]
pub struct AssetNotFoundError {
    asset: String,
}

impl std::fmt::Display for AssetNotFoundError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "Asset {} not found", self.asset)
    }
}

impl std::error::Error for AssetNotFoundError {}
