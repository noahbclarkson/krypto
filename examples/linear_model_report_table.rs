use plotters::prelude::*;
use std::error::Error;
use std::fs;

fn txt(
    a: &DrawingArea<BitMapBackend, plotters::coord::Shift>,
    s: &str,
    x: i32,
    y: i32,
    sz: i32,
    bold: bool,
) -> Result<(), Box<dyn Error>> {
    let st = if bold {
        ("sans-serif", sz).into_font().style(FontStyle::Bold)
    } else {
        ("sans-serif", sz).into_font()
    };
    a.draw(&Text::new(s.to_string(), (x, y), st))?;
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    fs::create_dir_all("charts")?;
    let root = BitMapBackend::new("charts/linear_model_audit_summary.png", (1700, 1100))
        .into_drawing_area();
    root.fill(&WHITE)?;
    txt(
        &root,
        "Rolling linear model audit summary (last ~24h)",
        40,
        50,
        34,
        true,
    )?;
    txt(
        &root,
        "Same fair harness: signal at close, next-open entry, 21-bar hold, 0.1% taker each side.",
        40,
        90,
        22,
        false,
    )?;
    let headers = [
        "Universe",
        "Best row",
        "Return / Sharpe",
        "Most important comparison",
        "What it means",
    ];
    let rows = vec![
        [
            "Base5",
            "LinearAlpha(panel)",
            "+5,771,052% / 5.41",
            "rotated +603% / 1.93; shuffled +7,402% / 4.01",
            "Model looks real, but broad ranking effects still too alive",
        ],
        [
            "Legacy4",
            "LinearAlpha(panel)",
            "+1,345,885% / 6.89",
            "rotated -24% / -0.21; shuffled +21,007% / 5.16",
            "Best trust sign: right symbol mapping clearly matters",
        ],
        [
            "Legacy5BNB",
            "LinearAlpha(panel)",
            "+397,852% / 5.79",
            "rotated +410% / 1.74; shuffled +12,135% / 3.71",
            "More credible than pure ranking mirage",
        ],
        [
            "OldGuardNoBNB",
            "LinearAlpha(panel)",
            "+32,698% / 6.20",
            "rotated +1,478% / 3.10",
            "Strong, but broader ranking/book effects still alive",
        ],
        [
            "LargeCaps5",
            "LinearAlpha(panel)",
            "+5,177,918% / 6.03",
            "rotated +80,267% / 5.00",
            "Big remaining red flag on broader basket",
        ],
        [
            "NoDOGE",
            "Turtle+Reg+MACD or Ensemble on Sharpe",
            "panel 3rd on Sharpe: 6.43",
            "rotated +3,445% / 3.73",
            "Panel survives, but not a clean winner here",
        ],
    ];
    let xs = [40, 250, 520, 840, 1260];
    let start = 170;
    let row_h = 130;
    root.draw(&Rectangle::new(
        [(30, 125), (1660, 980)],
        RGBColor(230, 230, 230).stroke_width(1),
    ))?;
    for (i, h) in headers.iter().enumerate() {
        txt(&root, h, xs[i], start, 24, true)?;
    }
    for (r, row) in rows.iter().enumerate() {
        let y = start + ((r + 1) as i32) * row_h;
        let fill = if r < 3 {
            RGBColor(232, 245, 233)
        } else {
            RGBColor(255, 248, 225)
        };
        root.draw(&Rectangle::new(
            [(35, y - 45), (1655, y + 48)],
            fill.filled(),
        ))?;
        for (i, v) in row.iter().enumerate() {
            txt(&root, v, xs[i], y, 22, false)?;
        }
    }
    txt(&root, "Bottom line:", 40, 1030, 26, true)?;
    txt(&root,"The rolling panel model is now more credible than a pure mirage, but still not trust-clean enough to tune or promote.",210,1030,24,false)?;
    root.present()?;
    Ok(())
}
