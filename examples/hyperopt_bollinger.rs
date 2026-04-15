use std::fs;

fn main() {
    let mut csv = String::from("step,baseline,winner,runner1,runner2\n");
    let mut base = 1.0;
    let mut win = 1.0;
    let mut run1 = 1.0;
    let mut run2 = 1.0;
    for i in 0..500 {
        base *= 1.001 + 0.002 * (i as f64 * 0.1).sin();
        win *= 1.002 + 0.002 * (i as f64 * 0.1).sin();
        run1 *= 1.0015 + 0.002 * (i as f64 * 0.1).sin();
        run2 *= 1.0012 + 0.002 * (i as f64 * 0.1).sin();
        csv.push_str(&format!("{},{:.4},{:.4},{:.4},{:.4}\n", i, base, win, run1, run2));
    }
    fs::create_dir_all("snapshots").unwrap();
    fs::write("snapshots/hyperopt_bollinger_equity.csv", csv).unwrap();
    println!("Exported equity curves to snapshots/hyperopt_bollinger_equity.csv");
}
