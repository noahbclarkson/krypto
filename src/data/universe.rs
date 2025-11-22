use super::loader::DataLoader;
use anyhow::Result;
use futures::future::join_all;
use polars::prelude::*;
use std::collections::HashMap;

pub struct Universe;

impl Universe {
    pub fn new() -> Self {
        Self
    }

    /// Fetches multiple symbols and intervals concurrently
    pub async fn fetch_universe(
        &self,
        symbols: &[&str],
        intervals: &[&str],
        limit: u16,
    ) -> Result<HashMap<String, DataFrame>> {
        let mut tasks = Vec::new();

        for &sym in symbols {
            for &inv in intervals {
                let key = format!("{sym}_{inv}");
                let sym_owned = sym.to_string();
                let inv_owned = inv.to_string();
                let loader = DataLoader::new(None, None);

                tasks.push(async move {
                    let df = loader.fetch_data(&sym_owned, &inv_owned, limit).await?;
                    Ok::<(String, DataFrame), anyhow::Error>((key, df))
                });
            }
        }

        let results = join_all(tasks).await;

        let mut map = HashMap::new();
        for res in results {
            match res {
                Ok((key, df)) => {
                    map.insert(key, df);
                }
                Err(e) => eprintln!("Failed to fetch data: {e}"),
            }
        }

        Ok(map)
    }
}

impl Default for Universe {
    fn default() -> Self {
        Self::new()
    }
}
