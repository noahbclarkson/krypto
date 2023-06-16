use std::fs::File;

use csv::Writer;

use crate::{config::Config, math::format_number, testing::TestData};

pub struct LiveData {
    pub test: TestData,
    pub csv: Writer<File>,
    pub original_config: Config,
    pub enter_price: Option<f64>,
    pub index: Option<usize>,
    pub last_score: f64,
    pub test_true: bool,
}

impl LiveData {
    pub fn new(starting_cash: f64, config: &Config, test_true: bool) -> LiveData {
        let test = TestData::new(starting_cash);
        let file_name = match test_true {
            true => "test.csv",
            false => "live.csv",
        };
        let mut file = csv::Writer::from_path(file_name)
            .unwrap_or_else(|_| panic!("Failed to create csv file {}", file_name));
        file.write_record(&[
            "Cash ($)",
            "Accuracy",
            "Correct/Incorrect",
            "Current Price",
            "Enter Price",
            "Score",
        ])
        .unwrap_or_else(|_| println!("Failed to write to csv file"));
        let original_config = config.clone();
        LiveData {
            test,
            csv: file,
            original_config,
            enter_price: None,
            index: None,
            last_score: 0.0,
            test_true,
        }
    }

    pub fn print(&self) {
        println!(
            "Cash: ${}, Accuracy: {:.2}%, Correct/Incorrect: {}/{}, Score {:.5}",
            format_number(*self.test.cash()),
            self.test.get_accuracy() * 100.0,
            self.test.correct(),
            self.test.incorrect(),
            self.last_score
        );
    }

    pub fn write(&mut self, current_price: f64, correct: bool) {
        self.csv
            .write_record(&[
                self.test.cash().to_string(),
                self.test.get_accuracy().to_string(),
                correct.to_string(),
                current_price.to_string(),
                self.enter_price.unwrap().to_string(),
                self.last_score.to_string(),
            ])
            .unwrap_or_else(|_| println!("Failed to write to csv file"));
        self.csv
            .flush()
            .unwrap_or_else(|_| println!("Failed to flush csv file"));
    }

    pub fn print_new_trade(&self, score: f64, ticker: &str) {
        if score > 0.0 {
            println!(
                "Enter Long: for {} at ${:.5}",
                ticker,
                self.enter_price.unwrap()
            );
        } else {
            println!("Error: Score is negative");
        }
    }
}
