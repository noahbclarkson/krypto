use actix_web::{web, App, HttpResponse, HttpServer, Responder};
use actix_cors::Cors;
use genevo::{
    ga::genetic_algorithm,
    operator::prelude::{ElitistReinserter, MaximizeSelector},
    prelude::{build_population, simulate, GenerationLimit, Population},
    simulation::{SimResult, Simulation, SimulationBuilder},
};
use krypto::{
    config::KryptoConfig,
    data::dataset::overall_dataset::Dataset,
    error::KryptoError,
    logging::setup_tracing,
    optimisation::{
        TradingStrategyCrossover, TradingStrategyFitnessFunction, TradingStrategyGenome,
        TradingStrategyGenomeBuilder, TradingStrategyMutation,
    },
};
use serde::{Deserialize, Serialize};
use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    path::PathBuf,
};
use binance::rest_model::OrderSide;
use std::sync::{Arc, Mutex};
use tokio::task;

#[derive(Debug, Serialize, Deserialize, Clone)]
struct TrainingStatus {
    running: bool,
    generation: u64,
    best_fitness: i64,
    done: bool,
    error: Option<String>,
}

impl TrainingStatus {
    fn new() -> Self {
        Self {
            running: false,
            generation: 0,
            best_fitness: 0,
            done: false,
            error: None,
        }
    }
}

#[derive(Debug, Serialize, Clone)]
struct GenerationSnapshot {
    generation: u64,
    fitness: i64,
    sharpe_ratio: f64,
}

#[derive(Debug, Serialize, Clone)]
struct EquityPoint {
    timestamp: chrono::DateTime<chrono::Utc>,
    equity: f64,
}

#[derive(Debug, Serialize, Clone)]
struct BestStrategyData {
    hash: u64,
    trade_log: Vec<krypto::algorithm::test_data::TradeLogEntry>,
    equity_curve: Vec<EquityPoint>,
}

async fn health() -> impl Responder {
    HttpResponse::Ok().body("ok")
}

async fn get_status(state: web::Data<AppState>) -> impl Responder {
    let status = state.status.lock().unwrap().clone();
    HttpResponse::Ok().json(status)
}

async fn get_config(state: web::Data<AppState>) -> impl Responder {
    let cfg = state.config.lock().unwrap().clone();
    HttpResponse::Ok().json(cfg)
}

async fn update_config(cfg: web::Json<KryptoConfig>, state: web::Data<AppState>) -> impl Responder {
    let mut current = state.config.lock().unwrap();
    *current = cfg.into_inner();
    HttpResponse::Ok().body("config updated")
}

async fn get_history(state: web::Data<AppState>) -> impl Responder {
    let history = state.history.lock().unwrap().clone();
    HttpResponse::Ok().json(history)
}

async fn get_best_hash(state: web::Data<AppState>) -> impl Responder {
    let best = state.best.lock().unwrap();
    let hash = best.as_ref().map(|b| b.hash);
    HttpResponse::Ok().json(hash)
}

async fn best_changed(path: web::Path<u64>, state: web::Data<AppState>) -> impl Responder {
    let client_hash = path.into_inner();
    let best = state.best.lock().unwrap();
    let changed = match *best {
        Some(ref b) => b.hash != client_hash,
        None => true,
    };
    HttpResponse::Ok().json(changed)
}

async fn get_best_data(state: web::Data<AppState>) -> impl Responder {
    let data = state.best.lock().unwrap().clone();
    match data {
        Some(d) => HttpResponse::Ok().json(d),
        None => HttpResponse::NoContent().finish(),
    }
}

async fn start_training(state: web::Data<AppState>) -> impl Responder {
    let mut status = state.status.lock().unwrap();
    if status.running {
        return HttpResponse::BadRequest().body("training already running");
    }
    status.running = true;
    status.done = false;
    status.error = None;
    status.generation = 0;
    status.best_fitness = 0;
    drop(status);

    let status_arc = state.status.clone();
    let history = state.history.clone();
    let best = state.best.clone();
    let cfg = state.config.lock().unwrap().clone();
    task::spawn(async move {
        let res = run_optimisation(status_arc.clone(), history, best, cfg).await;
        let mut st = status_arc.lock().unwrap();
        match res {
            Ok(_) => {
                st.running = false;
                st.done = true;
            }
            Err(e) => {
                st.running = false;
                st.error = Some(e.to_string());
                st.done = true;
            }
        }
    });

    HttpResponse::Ok().body("started")
}

struct AppState {
    status: Arc<Mutex<TrainingStatus>>,
    config: Arc<Mutex<KryptoConfig>>,
    history: Arc<Mutex<Vec<GenerationSnapshot>>>,
    best: Arc<Mutex<Option<BestStrategyData>>>,
}

async fn run_optimisation(
    status: Arc<Mutex<TrainingStatus>>,
    history: Arc<Mutex<Vec<GenerationSnapshot>>>,
    best: Arc<Mutex<Option<BestStrategyData>>>,
    config: KryptoConfig,
) -> Result<(), KryptoError> {
    let (_guard, _file_guard) = setup_tracing(Some("logs")).expect("log");
    let config = Arc::new(config);

    let dataset = Arc::new(Dataset::load(&config).await?);
    if dataset.is_empty() {
        return Ok(()); // nothing to do
    }

    let available_tickers = config.symbols.clone();
    let available_intervals = config.intervals.clone();
    let available_technicals = config.technicals.clone();
    config.validate()?;

    let fitness_function = TradingStrategyFitnessFunction::new(
        config.clone(),
        dataset.clone(),
        available_tickers.clone(),
        available_technicals.clone(),
    );

    let initial_population: Population<TradingStrategyGenome> = build_population()
        .with_genome_builder(TradingStrategyGenomeBuilder::new(
            available_tickers.clone(),
            available_intervals.clone(),
            available_technicals.clone(),
            config.max_depth,
            config.max_n,
        ))
        .of_size(config.population_size)
        .uniform_at_random();

    let ga = genetic_algorithm()
        .with_evaluation(fitness_function.clone())
        .with_selection(MaximizeSelector::new(
            config.selection_ratio,
            config.num_individuals_per_parents,
        ))
        .with_crossover(TradingStrategyCrossover {
            available_tickers: Arc::new(available_tickers.clone()),
        })
        .with_mutation(TradingStrategyMutation::new(
            config.mutation_rate,
            Arc::new(available_tickers.clone()),
            Arc::new(available_intervals.clone()),
            config.max_depth,
            config.max_n,
        ))
        .with_reinsertion(ElitistReinserter::new(
            fitness_function.clone(),
            true,
            config.reinsertion_ratio,
        ))
        .with_initial_population(initial_population)
        .build();

    let report_dir = krypto::optimisation::setup_report_dirs()?;

    let mut sim = simulate(ga)
        .until(GenerationLimit::new(config.generation_limit))
        .build();

    let mut best_overall = i64::MIN;

    loop {
        let result = sim.step();
        match result {
            Ok(SimResult::Intermediate(step)) => {
                let current_gen = step.iteration;
                let best_genome = &step.result.best_solution.solution.genome;
                let (fitness, metrics) = fitness_function.evaluate_genome(best_genome)?;
                let mut st = status.lock().unwrap();
                st.generation = current_gen;
                st.best_fitness = fitness;
                drop(st);

                history.lock().unwrap().push(GenerationSnapshot {
                    generation: current_gen,
                    fitness,
                    sharpe_ratio: metrics.sharpe_ratio,
                });

                if fitness > best_overall {
                    best_overall = fitness;
                    krypto::optimisation::generate_trade_log_for_best(
                        best_genome,
                        &report_dir,
                        &config,
                        &dataset,
                        &available_tickers,
                        &available_technicals,
                    )?;

                    let trade_log_path = report_dir.join("top").join("top-strategy-trades.csv");
                    let equity_path = report_dir.join("top").join("top-strategy-equity.csv");
                    let trade_log = read_trade_log_csv(&trade_log_path)?;
                    let equity_curve = read_equity_curve_csv(&equity_path)?;
                    let mut hasher = DefaultHasher::new();
                    best_genome.hash(&mut hasher);
                    let hash = hasher.finish();

                    let equity_curve = equity_curve
                        .into_iter()
                        .map(|(t, e)| EquityPoint { timestamp: t, equity: e })
                        .collect();

                    *best.lock().unwrap() = Some(BestStrategyData {
                        hash,
                        trade_log,
                        equity_curve,
                    });
                }
            }
            Ok(SimResult::Final(_, _, _, _)) => break,
            Err(e) => {
                return Err(KryptoError::FitnessCalculationError(format!(
                    "GA simulation failed: {}",
                    e
                )))
            }
        }
    }
    Ok(())
}

fn read_trade_log_csv(path: &PathBuf) -> Result<Vec<krypto::algorithm::test_data::TradeLogEntry>, KryptoError> {
    let mut rdr = csv::Reader::from_path(path)?;
    let mut log = Vec::new();
    for result in rdr.records() {
        let rec = result?;
        let timestamp = chrono::DateTime::parse_from_rfc3339(&rec[0])?
            .with_timezone(&chrono::Utc);
        let side = match rec[2].to_uppercase().as_str() {
            "BUY" => OrderSide::Buy,
            _ => OrderSide::Sell,
        };
        log.push(krypto::algorithm::test_data::TradeLogEntry {
            timestamp,
            symbol: rec[1].to_string(),
            side,
            entry_price: rec[3].parse().unwrap_or(0.0),
            exit_price: rec[4].parse().unwrap_or(0.0),
            quantity: rec[5].parse().unwrap_or(0.0),
            pnl: rec[6].parse().unwrap_or(0.0),
            pnl_pct: rec[7].parse().unwrap_or(0.0),
            fee: rec[8].parse().unwrap_or(0.0),
            cash_after_trade: rec[9].parse().unwrap_or(0.0),
            equity_after_trade: rec[10].parse().unwrap_or(0.0),
            reason: rec[11].to_string(),
        });
    }
    Ok(log)
}

fn read_equity_curve_csv(path: &PathBuf) -> Result<Vec<(chrono::DateTime<chrono::Utc>, f64)>, KryptoError> {
    let mut rdr = csv::Reader::from_path(path)?;
    let mut curve = Vec::new();
    for result in rdr.records() {
        let rec = result?;
        let timestamp = chrono::DateTime::parse_from_rfc3339(&rec[0])?.with_timezone(&chrono::Utc);
        let equity: f64 = rec[1].parse().unwrap_or(0.0);
        curve.push((timestamp, equity));
    }
    Ok(curve)
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let status = Arc::new(Mutex::new(TrainingStatus::new()));
    let history = Arc::new(Mutex::new(Vec::new()));
    let best = Arc::new(Mutex::new(None));
    let cfg = KryptoConfig::read_config::<&str>(None)
        .await
        .expect("Failed to load config");
    let app_state = web::Data::new(AppState {
        status,
        config: Arc::new(Mutex::new(cfg)),
        history,
        best,
    });

    HttpServer::new(move || {
        App::new()
            .wrap(Cors::permissive())
            .app_data(app_state.clone())
            .route("/health", web::get().to(health))
            .route("/status", web::get().to(get_status))
            .route("/config", web::get().to(get_config))
            .route("/config", web::post().to(update_config))
            .route("/train/start", web::post().to(start_training))
            .route("/generation", web::get().to(get_history))
            .route("/best/hash", web::get().to(get_best_hash))
            .route("/best/changed/{hash}", web::get().to(best_changed))
            .route("/best/data", web::get().to(get_best_data))
    })
    .bind(("0.0.0.0", 8080))?
    .run()
    .await
}
