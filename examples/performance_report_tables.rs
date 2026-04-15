use plotters::prelude::*;
use std::error::Error;
use std::fs;

fn main() -> Result<(), Box<dyn Error>> {
    fs::create_dir_all("charts")?;
    head_to_head_table()?;
    universe_winners_table()?;
    breadth_table()?;
    Ok(())
}

fn cell_text(
    area: &DrawingArea<BitMapBackend, plotters::coord::Shift>,
    text: &str,
    x: i32,
    y: i32,
    size: i32,
    bold: bool,
) -> Result<(), Box<dyn Error>> {
    let style = if bold {
        ("sans-serif", size).into_font().style(FontStyle::Bold)
    } else {
        ("sans-serif", size).into_font()
    };
    area.draw(&Text::new(text.to_string(), (x, y), style))?;
    Ok(())
}

fn head_to_head_table() -> Result<(), Box<dyn Error>> {
    let root = BitMapBackend::new("charts/strategy_performance_summary.png", (1600, 1000))
        .into_drawing_area();
    root.fill(&WHITE)?;
    cell_text(
        &root,
        "Strategy performance summary (current fair daily benchmark)",
        40,
        50,
        34,
        true,
    )?;
    cell_text(&root, "Same rules for all rows: signal at close, next-open entry, 21-bar hold, 0.1% taker each side.", 40, 90, 22, false)?;

    let headers = [
        "Rank",
        "Strategy",
        "Score",
        "Return",
        "Trades",
        "WF",
        "Resamples",
        "Universe wins",
        "Takeaway",
    ];
    let rows = vec![
        [
            "1",
            "MACD+Regime",
            "0.822",
            "+3961%",
            "469",
            "4/4",
            "15/15",
            "4/9",
            "Best all-around fair benchmark row",
        ],
        [
            "2",
            "MACD",
            "0.659",
            "+3540%",
            "683",
            "4/4",
            "15/15",
            "5/9",
            "Wins more old/legacy baskets",
        ],
        [
            "3",
            "Turtle+Regime",
            "0.556",
            "+3193%",
            "398",
            "4/4",
            "15/15",
            "0/9",
            "Chronology-clean, less dominant by basket",
        ],
        [
            "4",
            "Turtle+Regime+MACD",
            "0.523",
            "+3309%",
            "349",
            "4/4",
            "15/15",
            "0/9",
            "Strong modern-basket / capped-book rival",
        ],
        [
            "5",
            "Turtle+MACD",
            "0.517",
            "+3011%",
            "405",
            "4/4",
            "15/15",
            "0/9",
            "Solid but not leading",
        ],
    ];

    let col_x = [40, 120, 420, 530, 680, 790, 900, 1070, 1240];
    let row_h = 85;
    let start_y = 150;

    root.draw(&Rectangle::new(
        [(30, 115), (1570, 850)],
        RGBColor(230, 230, 230).stroke_width(1),
    ))?;
    for (i, h) in headers.iter().enumerate() {
        cell_text(&root, h, col_x[i], start_y, 24, true)?;
    }
    for (r, row) in rows.iter().enumerate() {
        let y = start_y + ((r + 1) as i32) * row_h;
        let fill = if r == 0 {
            RGBColor(232, 245, 233)
        } else {
            RGBColor(250, 250, 250)
        };
        root.draw(&Rectangle::new(
            [(35, y - 35), (1565, y + 30)],
            fill.filled(),
        ))?;
        for (i, v) in row.iter().enumerate() {
            cell_text(&root, v, col_x[i], y, 22, i == 1)?;
        }
    }

    cell_text(&root, "Human read:", 40, 900, 26, true)?;
    cell_text(&root, "• The project’s current leader is MACD+Regime, but it is not a clean final winner under every lens.", 40, 940, 24, false)?;
    cell_text(
        &root,
        "• Plain MACD is still dangerous to ignore because it wins more legacy-heavy universes.",
        40,
        975,
        24,
        false,
    )?;
    root.present()?;
    Ok(())
}

fn universe_winners_table() -> Result<(), Box<dyn Error>> {
    let root = BitMapBackend::new("charts/portfolio_performance_by_universe.png", (1700, 1050))
        .into_drawing_area();
    root.fill(&WHITE)?;
    cell_text(
        &root,
        "Portfolio performance by universe (top-3 capped book)",
        40,
        50,
        34,
        true,
    )?;
    cell_text(&root, "This is the more realistic portfolio lens: capped concentration, aligned positions, not just trade-sum headlines.", 40, 90, 22, false)?;

    let headers = [
        "Universe",
        "Best strategy",
        "Capped return",
        "Sharpe",
        "MaxDD",
        "What it means",
    ];
    let rows = vec![
        [
            "Base6",
            "Turtle+Regime+MACD",
            "+681,746%",
            "8.08",
            "32.9%",
            "Best broad modern-basket candidate",
        ],
        [
            "NoDOGE",
            "Turtle+Regime+MACD",
            "+114,741%",
            "7.25",
            "28.4%",
            "Still strongest on modern basket without DOGE",
        ],
        [
            "Legacy5",
            "MACD+Regime",
            "+10,483%",
            "3.63",
            "58.4%",
            "Filtered MACD leads older legacy basket",
        ],
        [
            "LargeCaps6",
            "Turtle+Regime+MACD",
            "+27,415%",
            "4.53",
            "52.4%",
            "Turtle combo stronger on larger modern caps",
        ],
        [
            "OldGuard6",
            "MACD+Regime",
            "+6,825%",
            "4.15",
            "49.2%",
            "Filtered MACD survives harsh old-guard basket best",
        ],
        [
            "OldGuardNoBNB",
            "MACD+Regime",
            "+3,325%",
            "4.59",
            "40.9%",
            "Removing BNB does not kill filtered-MACD edge",
        ],
    ];
    let col_x = [40, 260, 620, 860, 990, 1120];
    let row_h = 110;
    let start_y = 160;
    root.draw(&Rectangle::new(
        [(30, 120), (1665, 885)],
        RGBColor(230, 230, 230).stroke_width(1),
    ))?;
    for (i, h) in headers.iter().enumerate() {
        cell_text(&root, h, col_x[i], start_y, 24, true)?;
    }
    for (r, row) in rows.iter().enumerate() {
        let y = start_y + ((r + 1) as i32) * row_h;
        let fill = if row[1].contains("MACD+Regime") {
            RGBColor(232, 240, 254)
        } else {
            RGBColor(232, 245, 233)
        };
        root.draw(&Rectangle::new(
            [(35, y - 40), (1660, y + 40)],
            fill.filled(),
        ))?;
        for (i, v) in row.iter().enumerate() {
            cell_text(&root, v, col_x[i], y, 22, i == 1)?;
        }
    }
    cell_text(&root, "Human read:", 40, 950, 26, true)?;
    cell_text(&root, "• Modern / broader baskets favor Turtle+Regime+MACD. Older / survivorship-harsher baskets favor MACD+Regime.", 40, 990, 24, false)?;
    root.present()?;
    Ok(())
}

fn breadth_table() -> Result<(), Box<dyn Error>> {
    let root = BitMapBackend::new("charts/breadth_family_performance.png", (1700, 1050))
        .into_drawing_area();
    root.fill(&WHITE)?;
    cell_text(
        &root,
        "Broader search: what actually looked competitive",
        40,
        50,
        34,
        true,
    )?;
    cell_text(&root, "These are the main non-core families I tested under the same fair daily assumptions over the last few days.", 40, 90, 22, false)?;

    let headers = [
        "Family",
        "Best headline result",
        "Robustness read",
        "Current status",
    ];
    let rows = vec![
        [
            "Cross-sectional momentum",
            "+3172%, 4/4 WF, 14/15 resamples",
            "Competitive, but weakens on legacy baskets",
            "Worth keeping alive",
        ],
        [
            "Ensemble majority vote",
            "+3785% on Base5, won 3/6 universes",
            "Best breadth result so far; useful confirmation",
            "Worth deeper follow-up",
        ],
        [
            "Cross-impact / ticker interaction",
            "+1522% on OldGuardNoBNB, 15/15 resamples",
            "Interesting old-guard pocket, not universal",
            "Promising but basket-dependent",
        ],
        [
            "Leader-laggard",
            "+1953% on Base5",
            "Secondary family, not beating main leaders",
            "Interesting but not leading",
        ],
        [
            "Exogenous macro context",
            "Small chronology improvements in spots",
            "Mostly acted like blunt trade suppressors",
            "No clear edge yet",
        ],
        [
            "Funding / carry",
            "Carry exists, but too small",
            "Still weak after realized funding accounting",
            "Currently not good enough",
        ],
    ];
    let col_x = [40, 420, 860, 1280];
    let row_h = 115;
    let start_y = 170;
    root.draw(&Rectangle::new(
        [(30, 125), (1660, 930)],
        RGBColor(230, 230, 230).stroke_width(1),
    ))?;
    for (i, h) in headers.iter().enumerate() {
        cell_text(&root, h, col_x[i], start_y, 24, true)?;
    }
    for (r, row) in rows.iter().enumerate() {
        let y = start_y + ((r + 1) as i32) * row_h;
        let fill = if row[3].contains("Worth") {
            RGBColor(232, 245, 233)
        } else if row[3].contains("Promising") || row[3].contains("Interesting") {
            RGBColor(255, 243, 224)
        } else {
            RGBColor(255, 235, 238)
        };
        root.draw(&Rectangle::new(
            [(35, y - 42), (1655, y + 42)],
            fill.filled(),
        ))?;
        for (i, v) in row.iter().enumerate() {
            cell_text(&root, v, col_x[i], y, 22, false)?;
        }
    }
    cell_text(&root, "Human read:", 40, 980, 26, true)?;
    cell_text(&root, "• The search did broaden. The best non-trend results were cross-sectional momentum, ensembles, and some old-guard cross-impact pockets.", 40, 1015, 24, false)?;
    root.present()?;
    Ok(())
}
