use std::{error::Error, fs::File};

use csv::WriterBuilder;
use egui::{Shape, plot::{Line, PlotPoints, Plot}};
use krypto::{algorithm::Algorithm, config::*, historical_data::HistoricalData};

const MAX_DEPTH_TEST_END: usize = 25;
const MAX_DEPTH_TEST_START: usize = 2;
const MAX_MARGIN_TEST: f64 = 3.0;
const MAX_MINIMUM_SCORE_TEST: f64 = 0.01;
const MINIMUM_SCORE_STEP: f64 = 0.0001;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let (tickers, config) = get_configuration().await?;
    let data = load_data(&tickers, &config).await;
    let mut algorithm = Algorithm::new(data);
    algorithm.compute_relationships();
    println!("Computed the relationships successfully");
    let test = algorithm.test(&config);
    println!("Initial Test Result: ");
    println!("{}", test);
    let cash_history = test.cash_history();

    // let native_options = eframe::NativeOptions::default();
    // let app = App {
    //     cash_history: cash_history.clone(),
    // };
    // eframe::run_native(
    //     "My egui App",
    //     native_options,
    //     Box::new(|_cc| Box::new(app)),
    // )?;

    // let result = find_best_parameters(&mut algorithm, &config, &0.0);
    // match result {
    //     Ok(result) => println!("Best result: {}", result),
    //     Err(e) => eprintln!("Failed to find best parameters: {}", e),
    // }
    
    algorithm.live_test(&config, &tickers).await?;
    Ok(())
}

#[derive(Default)]
struct App {
    cash_history: Vec<f64>,
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Krypto");
            ui.separator();
            let pp: PlotPoints = self.cash_history.iter().enumerate().map(|(x, y)| [x as f64, *y]).collect();
            let line = Line::new(pp);
            Plot::new("Cash History").auto_bounds_y().show(ui, |plot_ui| plot_ui.line(line));
        });
    }
}

pub async fn get_configuration() -> Result<(Vec<String>, Config), Box<dyn Error>> {
    let (tickers_res, config_res) = tokio::join!(read_tickers(), read_config());
    let tickers = tickers_res.unwrap_or_else(|_| {
        eprintln!("Failed to read tickers, using default values.");
        Config::get_default_tickers()
            .iter()
            .map(|s| s.to_string())
            .collect()
    });
    let config = config_res.unwrap_or_else(|_| {
        eprintln!("Failed to read config, using default values.");
        Config::default()
    });
    println!("Read the tickers and config successfully");
    Ok((tickers, config))
}

async fn load_data(tickers: &Vec<String>, config: &Config) -> HistoricalData {
    // If data.json is present in the current directory, load the data from it
    if let Ok(_) = File::open("data.json") {
        let data = HistoricalData::deserialize_from_json("data.json");
        println!("Loaded the data from data.json successfully");
        return data;
    }
    let mut data = HistoricalData::new(tickers);
    data.load(config).await;
    data.calculate_technicals();
    println!("Loaded the data successfully");
    data
}

#[allow(dead_code)]
pub fn find_best_parameters(
    algorithm: &mut Algorithm,
    config: &Config,
    best_result: &f64,
) -> std::io::Result<f64> {
    let mut best_depth = 0;
    let mut best_margin = 0.0;
    let mut best_minimum_score = 0.0;
    let mut best_result = *best_result;
    let mut writer = WriterBuilder::new()
        .has_headers(false)
        .from_writer(File::create("results.csv")?);

    let mut header = vec![format!("Margin")];
    for score in 1..=(MAX_MINIMUM_SCORE_TEST / MINIMUM_SCORE_STEP) as usize {
        header.push(format!("{}", score as f64 * MINIMUM_SCORE_STEP));
    }
    writer.write_record(&header)?;

    for depth in MAX_DEPTH_TEST_START..=MAX_DEPTH_TEST_END {
        algorithm.settings.set_depth(depth);
        algorithm.compute_relationships();

        for margin in 3..=MAX_MARGIN_TEST as usize {
            let mut row = vec![format!("{}", margin)];
            for score in 1..=(MAX_MINIMUM_SCORE_TEST / MINIMUM_SCORE_STEP) as usize {
                algorithm.settings.set_margin(margin as f64);
                algorithm
                    .settings
                    .set_min_score(score as f64 * MINIMUM_SCORE_STEP);

                let result = algorithm.test(config);
                if *result.cash() > best_result {
                    best_result = *result.cash();
                    best_depth = depth;
                    best_margin = margin as f64;
                    best_minimum_score = score as f64 * MINIMUM_SCORE_STEP;
                    println!(
                        "New best result: {}, depth: {}, margin: {}, minimum score: {}",
                        best_result, best_depth, best_margin, best_minimum_score
                    );
                }
                row.push(format!("{}", *result.cash()));
            }
            writer.write_record(&row)?;
            writer.flush()?;
        }
    }
    algorithm.settings.set_depth(best_depth);
    algorithm.settings.set_margin(best_margin);
    algorithm.settings.set_min_score(best_minimum_score);
    writer.flush()?;
    Ok(best_result)
}
