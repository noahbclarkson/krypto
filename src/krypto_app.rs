use std::{
    error::Error,
    sync::{Arc, Mutex},
    thread,
};

use egui::{
    plot::{Line, Plot, PlotPoints},
    Slider,
};

use crate::{algorithm::Algorithm, config::Config, historical_data::HistoricalData};

pub struct KryptoApp {
    algorithm: Arc<Mutex<Algorithm>>,
    data: Arc<Mutex<HistoricalData>>,
    config: Arc<Config>,
    tickers: Arc<Vec<String>>,
    depth_setting: usize,
    min_score_setting: f32,
}

impl KryptoApp {
    fn load_data_from_file(&mut self) {
        self.data = thread::spawn(move || {
            let data =
                futures::executor::block_on(HistoricalData::deserialize_from_csvs()).unwrap();
            println!("Loaded data");
            Arc::new(Mutex::new(data))
        })
        .join()
        .unwrap();
    }

    fn load_data_from_api(&mut self) {
        let tickers = Arc::clone(&self.tickers);
        let config = Arc::clone(&self.config);
        self.data = thread::spawn(move || {
            let mut data = HistoricalData::new(tickers.as_ref());
            // Use tokio to run data.load(&config) but don't block the main thread
            tokio::runtime::Runtime::new()
                .unwrap()
                .block_on(data.load(&config, None))
                .unwrap();
            data.calculate_candlestick_technicals().unwrap();
            data.normalize_technicals();
            println!("Loaded data");
            Arc::new(Mutex::new(data))
        })
        .join()
        .unwrap();
    }

    fn serialize_data(&mut self) {
        let data = Arc::clone(&self.data);
        thread::spawn(move || {
            let data = data.lock().unwrap();
            tokio::runtime::Runtime::new()
                .unwrap()
                .block_on(data.serialize_to_csvs())
                .unwrap();
            println!("Serialized data");
        });
    }

    fn calculate_relationships(&mut self) {
        let data = Arc::clone(&self.data);
        let algorithm = Arc::clone(&self.algorithm);
        thread::spawn(move || {
            let data = data.lock().unwrap();
            let candles = data.candles();
            tokio::runtime::Runtime::new()
                .unwrap()
                .block_on(algorithm.lock().unwrap().compute_relationships(candles));
            println!("Calculated relationships");
        });
    }

    fn serialize_algorithm(&self) {
        self.algorithm.lock().unwrap().serialize_to_json().unwrap();
    }
}

impl Default for KryptoApp {
    fn default() -> Self {
        let (tickers, config) = thread::spawn(move || {
            let (tickers, config) = futures::executor::block_on(get_configuration()).unwrap();
            println!("Got tickers and config");
            (tickers, config)
        })
        .join()
        .unwrap();
        let data = HistoricalData::new(&tickers);
        let algorithm = Algorithm::new(&config);
        let cfg = config.clone();
        KryptoApp {
            algorithm: Arc::new(Mutex::new(algorithm)),
            data: Arc::new(Mutex::new(data)),
            config: Arc::new(config),
            tickers: Arc::new(tickers),
            depth_setting: *cfg.depth(),
            min_score_setting: cfg.min_score().unwrap_or(0.0),
        }
    }
}

impl eframe::App for KryptoApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::SidePanel::left("side_panel")
            .default_width(200.0)
            .show(ctx, |ui| {
                ui.heading("Krypto");
                ui.separator();
                ui.heading("Data");
                if ui.button("Load Data From File").clicked() {
                    self.load_data_from_file();
                }
                if ui.button("Load Data From API").clicked() {
                    self.load_data_from_api();
                }
                if ui.button("Serialize Data").clicked() {
                    self.serialize_data();
                }
                ui.separator();
                ui.heading("Algorithm");
                if ui.button("Calculate Relationships").clicked() {
                    self.calculate_relationships();
                }
                if ui.button("Serialize Algorithm").clicked() {
                    self.serialize_algorithm();
                }
                ui.separator();
                ui.heading("Testing");
                if ui.button("Test").clicked() {
                    self.algorithm
                        .lock()
                        .unwrap()
                        .test(self.data.lock().unwrap().candles());
                }
                let algo = self.algorithm.lock().unwrap();
                let test_data = algo.test_data();
                if test_data.is_some() {
                    ui.heading("Test Results");
                    let test_data = test_data.clone().unwrap();
                    ui.label(format!("{}", test_data));
                    let map = test_data.calculate_average_returns(
                        self.config.clone().get_interval_minutes().unwrap() as usize,
                        *algo.settings().depth(),
                    );
                    for (k, v) in map {
                        ui.label(format!("{:.3}% {}", v, k));
                    }
                }
                ui.separator();
                ui.heading("Configuration");
                ui.add(Slider::new(&mut self.depth_setting, 1..=15).text("Depth"));
                ui.add(
                    Slider::new(&mut self.min_score_setting, 0.0..=50.0)
                        .text("Min Score")
                        .clamp_to_range(true),
                );
            });
        self.algorithm
            .lock()
            .unwrap()
            .settings_mut()
            .set_depth(self.depth_setting);
        self.algorithm
            .lock()
            .unwrap()
            .settings_mut()
            .set_min_score(Some(self.min_score_setting));
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Cash Balance ($)");
            let algo = self.algorithm.lock().unwrap();
            let test_data = algo.test_data();
            if test_data.is_some() {
                let test_data = test_data.clone().unwrap();
                let cash_history = test_data.cash_history();
                let pp: PlotPoints = cash_history
                    .iter()
                    .enumerate()
                    .map(|(i, cash)| [i as f64, cash.log10() as f64])
                    .collect();
                let line = Line::new(pp);
                Plot::new("cash_history").show(ui, |plot_ui| plot_ui.line(line));
            }
        });
    }
}

pub async fn get_configuration() -> Result<(Vec<String>, Config), Box<dyn Error>> {
    let (tickers_res, config_res) = tokio::join!(Config::read_tickers(), Config::read_config());
    println!("Read tickers and config");
    Ok((tickers_res?, config_res?))
}
