use calendar_slam_shared::{run_points, Attribute, AttributePickDto, SlamResultDto};
use js_sys::Array;
use wasm_bindgen::{closure::Closure, JsCast, JsValue};
use web_sys::{
    Blob, CanvasRenderingContext2d, File, FilePropertyBag, HtmlAnchorElement, HtmlCanvasElement,
    Url,
};

const WIDTH: u32 = 1080;
const HEIGHT: u32 = 1350;
const INK: &str = "#161616";
const PAPER: &str = "#f7f0df";
const COURT: &str = "#2f6f62";
const CLAY: &str = "#c94f31";
const LIME: &str = "#d7ff5f";
const CREAM: &str = "#fffaf0";

pub struct RunSummary<'a> {
    pub nickname: &'a str,
    pub picks: &'a [AttributePickDto],
    pub slams: &'a [SlamResultDto],
    pub overall: u8,
    pub language: ShareLanguage,
}

#[derive(Clone, Copy)]
pub enum ShareLanguage {
    Pt,
    En,
}

pub fn draw_summary(canvas: &HtmlCanvasElement, run: &RunSummary) -> Result<(), JsValue> {
    canvas.set_width(WIDTH);
    canvas.set_height(HEIGHT);

    let ctx = context(canvas)?;
    fill_rect(&ctx, PAPER, 0.0, 0.0, WIDTH as f64, HEIGHT as f64);

    fill_rect(&ctx, COURT, 42.0, 42.0, 996.0, 260.0);
    fill_rect(&ctx, CLAY, 42.0, 248.0, 996.0, 54.0);
    stroke_rect(&ctx, INK, 42.0, 42.0, 996.0, 260.0, 6.0);

    text(&ctx, LIME, "bold 34px Verdana", "CALENDARSLAM", 78.0, 98.0);
    text(
        &ctx,
        "#fff7e7",
        "bold 72px Georgia",
        summary_name(run.nickname, run.language),
        78.0,
        178.0,
    );

    let slams_won = run.slams.iter().filter(|slam| slam.won).count();
    metric(&ctx, "OVERALL", &run.overall.to_string(), 78.0, 226.0);
    metric(&ctx, "ATP PTS", &run_points(run.slams).to_string(), 360.0, 226.0);
    metric(&ctx, "SLAMS", &format!("{slams_won}/4"), 642.0, 226.0);

    text(&ctx, INK, "bold 34px Verdana", share_text(run.language, "attributes"), 58.0, 374.0);
    for (index, attribute) in Attribute::ALL.iter().enumerate() {
        let col = index % 2;
        let row = index / 2;
        let x = 58.0 + (col as f64 * 492.0);
        let y = 405.0 + (row as f64 * 154.0);
        let pick = run.picks.iter().find(|pick| pick.attribute == *attribute);
        attribute_card(&ctx, run.language, *attribute, pick, x, y);
    }

    text(&ctx, INK, "bold 34px Verdana", "Grand Slams", 58.0, 1062.0);
    for (index, slam) in run.slams.iter().enumerate() {
        let x = 58.0 + (index as f64 * 246.0);
        slam_card(&ctx, run.language, slam, x, 1094.0);
    }

    text(
        &ctx,
        "rgba(22, 22, 22, 0.62)",
        "24px Verdana",
        "calendarslam",
        58.0,
        1300.0,
    );

    Ok(())
}

pub fn download_png(canvas: &HtmlCanvasElement) -> Result<(), JsValue> {
    let data_url = canvas.to_data_url_with_type("image/png")?;
    let document = web_sys::window()
        .and_then(|window| window.document())
        .ok_or_else(|| JsValue::from_str("document unavailable"))?;
    let anchor: HtmlAnchorElement = document.create_element("a")?.dyn_into()?;
    anchor.set_href(&data_url);
    anchor.set_download("calendarslam.png");
    let body = document
        .body()
        .ok_or_else(|| JsValue::from_str("document body unavailable"))?;
    body.append_child(&anchor)?;
    anchor.click();
    anchor.remove();
    Ok(())
}

pub fn share_png(canvas: &HtmlCanvasElement) -> Result<(), JsValue> {
    let canvas_for_fallback = canvas.clone();
    let callback = Closure::once(move |blob: Option<Blob>| {
        let Some(blob) = blob else {
            let _ = download_png(&canvas_for_fallback);
            return;
        };
        if share_blob(blob).is_err() {
            let _ = download_png(&canvas_for_fallback);
        }
    });

    canvas.to_blob_with_type(callback.as_ref().unchecked_ref(), "image/png")?;
    callback.forget();
    Ok(())
}

fn share_blob(blob: Blob) -> Result<(), JsValue> {
    let window = web_sys::window().ok_or_else(|| JsValue::from_str("window unavailable"))?;
    let navigator = window.navigator();

    let file_parts = Array::new();
    file_parts.push(&blob);
    let file_options = FilePropertyBag::new();
    file_options.set_type("image/png");
    let file = File::new_with_blob_sequence_and_options(
        &file_parts,
        "calendarslam.png",
        &file_options,
    )?;

    let files = Array::new();
    files.push(&file);

    let data = web_sys::ShareData::new();
    data.set_title("CalendarSlam");
    data.set_text("Minha run no CalendarSlam");
    data.set_files(&files);

    if navigator.can_share_with_data(&data) {
        let _promise = navigator.share_with_data(&data);
        Ok(())
    } else {
        fallback_blob_download(blob)
    }
}

fn fallback_blob_download(blob: Blob) -> Result<(), JsValue> {
    let document = web_sys::window()
        .and_then(|window| window.document())
        .ok_or_else(|| JsValue::from_str("document unavailable"))?;
    let anchor: HtmlAnchorElement = document.create_element("a")?.dyn_into()?;
    let url = Url::create_object_url_with_blob(&blob)?;
    anchor.set_href(&url);
    anchor.set_download("calendarslam.png");
    let body = document
        .body()
        .ok_or_else(|| JsValue::from_str("document body unavailable"))?;
    body.append_child(&anchor)?;
    anchor.click();
    anchor.remove();
    Url::revoke_object_url(&url)
}

fn context(canvas: &HtmlCanvasElement) -> Result<CanvasRenderingContext2d, JsValue> {
    canvas
        .get_context("2d")?
        .ok_or_else(|| JsValue::from_str("2d context unavailable"))?
        .dyn_into()
        .map_err(Into::into)
}

fn fill_rect(ctx: &CanvasRenderingContext2d, color: &str, x: f64, y: f64, w: f64, h: f64) {
    ctx.set_fill_style_str(color);
    ctx.fill_rect(x, y, w, h);
}

fn stroke_rect(
    ctx: &CanvasRenderingContext2d,
    color: &str,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    line_width: f64,
) {
    ctx.set_stroke_style_str(color);
    ctx.set_line_width(line_width);
    ctx.stroke_rect(x, y, w, h);
}

fn text(ctx: &CanvasRenderingContext2d, color: &str, font: &str, value: &str, x: f64, y: f64) {
    ctx.set_fill_style_str(color);
    ctx.set_font(font);
    let _ = ctx.fill_text(value, x, y);
}

fn metric(ctx: &CanvasRenderingContext2d, label: &str, value: &str, x: f64, y: f64) {
    text(ctx, LIME, "bold 26px Verdana", label, x, y);
    text(ctx, "#fff7e7", "bold 58px Verdana", value, x, y + 62.0);
}

fn attribute_card(
    ctx: &CanvasRenderingContext2d,
    language: ShareLanguage,
    attribute: Attribute,
    pick: Option<&AttributePickDto>,
    x: f64,
    y: f64,
) {
    fill_rect(ctx, CREAM, x, y, 462.0, 126.0);
    stroke_rect(ctx, INK, x, y, 462.0, 126.0, 3.0);
    text(ctx, COURT, "bold 22px Verdana", attr_label(language, attribute), x + 20.0, y + 34.0);

    match pick {
        Some(pick) => {
            text(
                ctx,
                CLAY,
                "bold 52px Verdana",
                &pick.rating.to_string(),
                x + 20.0,
                y + 94.0,
            );
            text(ctx, INK, "bold 25px Verdana", &fit(&pick.player, 27), x + 108.0, y + 72.0);
            text(
                ctx,
                "rgba(22, 22, 22, 0.62)",
                "20px Verdana",
                &fit(&format!("{} {}", pick.source.tournament, pick.source.year), 32),
                x + 108.0,
                y + 102.0,
            );
        }
        None => text(ctx, "rgba(22, 22, 22, 0.45)", "28px Verdana", "--", x + 20.0, y + 88.0),
    }
}

fn slam_card(ctx: &CanvasRenderingContext2d, language: ShareLanguage, slam: &SlamResultDto, x: f64, y: f64) {
    fill_rect(ctx, if slam.won { LIME } else { CREAM }, x, y, 214.0, 150.0);
    stroke_rect(ctx, INK, x, y, 214.0, 150.0, 3.0);
    text(ctx, INK, "bold 24px Verdana", &slam.slam, x + 16.0, y + 36.0);
    text(ctx, CLAY, "bold 34px Verdana", &format!("{} pts", slam.points), x + 16.0, y + 84.0);
    let outcome = if slam.won {
        share_text(language, "champion").to_string()
    } else {
        format!("{} {}", share_text(language, "out"), slam.exit_round)
    };
    text(ctx, INK, "21px Verdana", &fit(&outcome, 16), x + 16.0, y + 122.0);
}

fn summary_name(nickname: &str, language: ShareLanguage) -> &str {
    let trimmed = nickname.trim();
    if trimmed.is_empty() {
        share_text(language, "my_run")
    } else {
        trimmed
    }
}

fn share_text(language: ShareLanguage, key: &str) -> &'static str {
    match language {
        ShareLanguage::Pt => match key {
            "attributes" => "Atributos",
            "champion" => "Campeao",
            "out" => "Saiu",
            "my_run" => "Minha run",
            _ => "",
        },
        ShareLanguage::En => match key {
            "attributes" => "Attributes",
            "champion" => "Champion",
            "out" => "Out",
            "my_run" => "My run",
            _ => "",
        },
    }
}

fn attr_label(language: ShareLanguage, attribute: Attribute) -> &'static str {
    match language {
        ShareLanguage::Pt => attribute.label(),
        ShareLanguage::En => match attribute {
            Attribute::Serve => "Serve",
            Attribute::Forehand => "Forehand",
            Attribute::Backhand => "Backhand",
            Attribute::Return => "Return",
            Attribute::Movement => "Movement",
            Attribute::Mental => "Mental",
            Attribute::Net => "Net play",
            Attribute::Stamina => "Stamina",
        },
    }
}

fn fit(value: &str, max_chars: usize) -> String {
    let count = value.chars().count();
    if count <= max_chars {
        value.to_string()
    } else {
        let mut fitted = value.chars().take(max_chars.saturating_sub(1)).collect::<String>();
        fitted.push_str("...");
        fitted
    }
}
