use plotters::prelude::*;
use std::error::Error;
use std::fs;

fn main() -> Result<(), Box<dyn Error>> {
    fs::create_dir_all("charts")?;
    progress_timeline()?;
    lens_comparison()?;
    breadth_scan()?;
    Ok(())
}

fn progress_timeline() -> Result<(), Box<dyn Error>> {
    let root = BitMapBackend::new("charts/progress_timeline.png", (1400, 900)).into_drawing_area();
    root.fill(&WHITE)?;
    let areas = root.split_evenly((2, 1));

    let top = &areas[0];
    let bottom = &areas[1];

    let sessions = vec![
        ("Mar24 early", 5, 1, 0),
        ("Mar24 late", 2, 1, 1),
        ("Mar25", 2, 3, 1),
        ("Mar26", 5, 3, 0),
        ("Mar27-29", 3, 8, 2),
    ];

    let mut chart = ChartBuilder::on(top)
        .caption(
            "Krypto progress over the last few days",
            ("sans-serif", 30).into_font(),
        )
        .margin(20)
        .x_label_area_size(50)
        .y_label_area_size(60)
        .build_cartesian_2d(0..sessions.len() as i32, 0..14i32)?;

    chart
        .configure_mesh()
        .disable_mesh()
        .y_desc("Research cycles")
        .x_desc("Time block")
        .x_labels(sessions.len())
        .x_label_formatter(&|x| {
            let idx = (*x as usize).min(sessions.len().saturating_sub(1));
            sessions[idx].0.to_string()
        })
        .draw()?;

    for (i, (_label, trust, breadth, stress)) in sessions.iter().enumerate() {
        let x = i as i32;
        let mut y0 = 0;
        let blocks = vec![
            (*trust, RGBColor(55, 126, 184), "Trust / harness"),
            (*breadth, RGBColor(77, 175, 74), "New strategy families"),
            (*stress, RGBColor(255, 127, 0), "Stress current leaders"),
        ];
        for (h, color, _) in blocks {
            chart.draw_series(std::iter::once(Rectangle::new(
                [(x, y0), (x + 1, y0 + h)],
                color.filled(),
            )))?;
            y0 += h;
        }
        chart.draw_series(std::iter::once(Text::new(
            format!("{} cycles", y0),
            (x, y0 + 1),
            ("sans-serif", 16).into_font(),
        )))?;
    }

    top.draw(&Text::new("Blue = build trust in backtests/harness | Green = explore new model families | Orange = stress current leaders",
        (30, 25), ("sans-serif", 20).into_font()))?;

    let notes = vec![
        "1) Mar 24: shifted from over-focus on MACD into a 3-track program.",
        "2) Biggest trust win: fixed slice-context / warmup bug in chronology windows.",
        "3) Mar 25-29: broadened into cross-sectional, leader-laggard, exogenous, ensemble, carry, and cross-impact families.",
        "4) Result: progress was real, but no single strategy is clean enough to promote yet.",
    ];
    for (i, note) in notes.iter().enumerate() {
        bottom.draw(&Text::new(
            note.to_string(),
            (30, 50 + i as i32 * 60),
            ("sans-serif", 24).into_font(),
        ))?;
    }
    bottom.draw(&Text::new(
        "Main project lesson: the work got more trustworthy, not just more profitable-looking.",
        (30, 320),
        ("sans-serif", 26).into_font().style(FontStyle::Bold),
    ))?;

    root.present()?;
    Ok(())
}

fn lens_comparison() -> Result<(), Box<dyn Error>> {
    let root = BitMapBackend::new("charts/progress_leaderboard_lenses.png", (1400, 900))
        .into_drawing_area();
    root.fill(&WHITE)?;
    let areas = root.split_evenly((2, 1));
    let top = &areas[0];
    let bottom = &areas[1];

    let candidates = ["MACD", "MACD+Regime", "Turtle+Reg+MACD"];
    let scores = [22, 29, 36];
    let sharpe_scores = [18, 30, 36];
    let chrono_scores = [31, 15, 30];

    let mut chart = ChartBuilder::on(top)
        .caption(
            "Why no trend strategy has been promoted yet",
            ("sans-serif", 30).into_font(),
        )
        .margin(20)
        .x_label_area_size(50)
        .y_label_area_size(60)
        .build_cartesian_2d(0..9i32, 0..40i32)?;

    chart
        .configure_mesh()
        .disable_mesh()
        .y_desc("Relative score (higher is better)")
        .draw()?;

    for i in 0..3 {
        let base_x = (i * 3) as i32;
        chart.draw_series(std::iter::once(Rectangle::new(
            [(base_x, 0), (base_x + 1, scores[i])],
            RGBColor(55, 126, 184).filled(),
        )))?;
        chart.draw_series(std::iter::once(Rectangle::new(
            [(base_x + 1, 0), (base_x + 2, sharpe_scores[i])],
            RGBColor(77, 175, 74).filled(),
        )))?;
        chart.draw_series(std::iter::once(Rectangle::new(
            [(base_x + 2, 0), (base_x + 3, chrono_scores[i])],
            RGBColor(255, 127, 0).filled(),
        )))?;
        chart.draw_series(std::iter::once(Text::new(
            candidates[i],
            (base_x + 1, -2),
            ("sans-serif", 18).into_font(),
        )))?;
    }

    top.draw(&Text::new("Blue = integrated benchmark score | Green = portfolio/Sharpe style lens | Orange = chronology stability lens",
        (40, 30), ("sans-serif", 20).into_font()))?;

    let notes = vec![
        "• MACD+Regime often wins the fair composite benchmark and harsher legacy baskets.",
        "• Turtle+Regime+MACD often wins the modern-basket capped-book / risk-adjusted view.",
        "• Plain MACD quietly kept strong chronology counts after the slice-context bug was fixed.",
        "• That means the apparent leader changes depending on the lens — exactly why nothing is promoted yet.",
    ];
    for (i, note) in notes.iter().enumerate() {
        bottom.draw(&Text::new(
            note.to_string(),
            (30, 55 + i as i32 * 60),
            ("sans-serif", 24).into_font(),
        ))?;
    }
    root.present()?;
    Ok(())
}

fn breadth_scan() -> Result<(), Box<dyn Error>> {
    let root =
        BitMapBackend::new("charts/progress_breadth_scan.png", (1400, 950)).into_drawing_area();
    root.fill(&WHITE)?;

    let families = vec![
        (
            "Cross-sectional\nmomentum",
            80,
            "Competitive, but weaker on legacy baskets",
        ),
        (
            "Ensemble\nmajority vote",
            88,
            "Best new breadth result; adds useful confirmation",
        ),
        (
            "Exogenous\nmacro gates",
            35,
            "Mostly acted as blunt trade suppressors",
        ),
        (
            "Funding / carry",
            12,
            "Structurally interesting, but still weak",
        ),
        (
            "Leader-laggard",
            28,
            "Secondary family, not a main edge yet",
        ),
        (
            "Cross-impact",
            46,
            "Interesting on old-guard baskets, not universal",
        ),
    ];

    let mut chart = ChartBuilder::on(&root)
        .caption(
            "What the broader search found",
            ("sans-serif", 30).into_font(),
        )
        .margin(20)
        .x_label_area_size(60)
        .y_label_area_size(70)
        .build_cartesian_2d(0..families.len() as i32, 0..100i32)?;

    chart
        .configure_mesh()
        .disable_mesh()
        .y_desc("How promising the family currently looks")
        .x_labels(families.len())
        .x_label_formatter(&|x| {
            let idx = (*x as usize).min(families.len().saturating_sub(1));
            families[idx].0.to_string()
        })
        .draw()?;

    for (i, (_name, score, blurb)) in families.iter().enumerate() {
        let color = if *score >= 75 {
            RGBColor(77, 175, 74)
        } else if *score >= 40 {
            RGBColor(255, 127, 0)
        } else {
            RGBColor(228, 26, 28)
        };
        chart.draw_series(std::iter::once(Rectangle::new(
            [(i as i32, 0), (i as i32 + 1, *score)],
            color.filled(),
        )))?;
        chart.draw_series(std::iter::once(Text::new(
            format!("{}", score),
            (i as i32, *score + 4),
            ("sans-serif", 18).into_font(),
        )))?;
        root.draw(&Text::new(
            blurb.to_string(),
            (60 + i as i32 * 220, 820),
            ("sans-serif", 18).into_font(),
        ))?;
    }
    root.draw(&Text::new(
        "Green = worth deeper follow-up | Orange = interesting but secondary | Red = structurally plausible but currently weak",
        (40, 40),
        ("sans-serif", 22).into_font(),
    ))?;
    root.present()?;
    Ok(())
}
