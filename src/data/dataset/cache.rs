use std::{
    fs::{self, File},
    io::{BufReader, BufWriter}, // Re-add BufWriter usage
    path::PathBuf,
};

use tracing::{debug, info, warn};

use crate::{config::KryptoConfig, data::interval::Interval, error::KryptoError};

use super::symbol_data::RawSymbolData;

const CACHE_FILE_EXT: &str = "bin";
const CACHE_VERSION: &str = "v1"; // Increment if cache format changes

/// Generates the expected cache file path for a given symbol and interval.
fn get_cache_path(
    config: &KryptoConfig,
    symbol: &str,
    interval: &Interval,
) -> Result<PathBuf, KryptoError> {
    let cache_dir = config.cache_dir.as_ref().ok_or_else(|| {
        KryptoError::ConfigError("Cache directory is not configured.".to_string())
    })?;

    // Include version in filename to invalidate old caches automatically
    let filename = format!(
        "{}_{}_{}.{}",
        symbol, interval, CACHE_VERSION, CACHE_FILE_EXT
    );
    Ok(cache_dir.join(filename))
}

/// Loads `RawSymbolData` from the cache if enabled, valid, and present.
pub fn load_from_cache(
    config: &KryptoConfig,
    symbol: &str,
    interval: &Interval,
) -> Result<Option<RawSymbolData>, KryptoError> {
    if !config.cache_enabled {
        return Ok(None);
    }

    let cache_path = get_cache_path(config, symbol, interval)?;

    if !cache_path.exists() {
        debug!("Cache miss (file not found): {}", cache_path.display());
        return Ok(None);
    }

    // Optional: Add cache expiry check (e.g., invalidate cache older than X days)
    // let metadata = fs::metadata(&cache_path)?;
    // let modified_time = metadata.modified()?;
    // let expiry_duration = Duration::days(7); // Example: 7 days expiry
    // if SystemTime::now().duration_since(modified_time)? > expiry_duration.to_std().unwrap() {
    //     info!("Cache miss (expired): {}", cache_path.display());
    //     return Ok(None);
    // }

    debug!("Attempting to load from cache: {}", cache_path.display());
    match File::open(&cache_path) {
        Ok(file) => {
            let mut reader = BufReader::new(file);
            match bincode::serde::decode_from_std_read(&mut reader, bincode::config::standard()) {
                Ok(data) => {
                    info!(
                        "Cache hit: Successfully loaded data for {} {} from {}",
                        symbol,
                        interval,
                        cache_path.display()
                    );
                    Ok(Some(data))
                }
                Err(e) => {
                    warn!(
                        "Failed to deserialize cache file {}: {}. Ignoring cache.",
                        cache_path.display(),
                        e
                    );
                    // Optionally delete the corrupted cache file
                    // let _ = fs::remove_file(&cache_path);
                    Ok(None) // Treat as cache miss
                }
            }
        }
        Err(e) => {
            warn!(
                "Failed to open cache file {}: {}. Ignoring cache.",
                cache_path.display(),
                e
            );
            Ok(None) // Treat as cache miss
        }
    }
}

/// Saves `RawSymbolData` to the cache if enabled.
pub fn save_to_cache(
    config: &KryptoConfig,
    symbol: &str,
    interval: &Interval,
    data: &RawSymbolData,
) -> Result<(), KryptoError> {
    if !config.cache_enabled {
        return Ok(());
    }

    let cache_path = get_cache_path(config, symbol, interval)?;

    debug!("Attempting to save to cache: {}", cache_path.display());

    // Ensure parent directory exists (should be handled in config loading, but double check)
    if let Some(parent_dir) = cache_path.parent() {
        if !parent_dir.exists() {
            fs::create_dir_all(parent_dir)?;
        }
    }

    match File::create(&cache_path) {
        Ok(file) => {
            let mut buffered_writer = BufWriter::new(file); // Use BufWriter again and make it mutable
            match bincode::serde::encode_into_std_write(
                data,
                &mut buffered_writer, // &mut still required
                bincode::config::standard(),
            ) {
                // Pass &mut writer
                Ok(_) => {
                    debug!(
                        "Successfully saved data for {} {} to cache: {}",
                        symbol,
                        interval,
                        cache_path.display()
                    );
                    Ok(())
                }
                Err(e) => {
                    warn!(
                        "Failed to serialize data to cache file {}: {}",
                        cache_path.display(),
                        e
                    );
                    // Don't error out, just log the warning
                    Ok(())
                }
            }
        }
        Err(e) => {
            warn!(
                "Failed to create cache file {}: {}",
                cache_path.display(),
                e
            );
            // Don't error out, just log the warning
            Ok(())
        }
    }
}
