use calendar_slam_shared::{EditionDto, LeaderboardRow, Level, RunDto, SavedRunDto, SpinDto};
use gloo_net::http::Request;

pub async fn spin(level: Option<Level>, tournament: Option<&str>) -> Result<SpinDto, String> {
    let mut url = "/api/spin".to_string();
    let mut params = Vec::new();

    if let Some(level) = level {
        params.push(format!("level={}", level_param(level)));
    }
    if let Some(tournament) = tournament {
        params.push(format!("tournament={}", encode(tournament)));
    }
    if !params.is_empty() {
        url.push('?');
        url.push_str(&params.join("&"));
    }

    Request::get(&url)
        .send()
        .await
        .map_err(|err| err.to_string())?
        .json()
        .await
        .map_err(|err| err.to_string())
}

pub async fn reroll(
    kind: &str,
    level: Level,
    tournament: Option<&str>,
    year: Option<i32>,
) -> Result<SpinDto, String> {
    let mut params = vec![
        format!("kind={kind}"),
        format!("level={}", level_param(level)),
    ];
    if let Some(tournament) = tournament {
        params.push(format!("tournament={}", encode(tournament)));
    }
    if let Some(year) = year {
        params.push(format!("year={year}"));
    }
    let url = format!("/api/reroll?{}", params.join("&"));

    let response = Request::get(&url).send().await.map_err(|err| err.to_string())?;
    if !response.ok() {
        return Err(format!("reroll status {}", response.status()));
    }
    response.json().await.map_err(|err| err.to_string())
}

pub async fn slam_edition(slam: &str) -> Result<EditionDto, String> {
    Request::get(&format!("/api/slam-edition?slam={slam}"))
        .send()
        .await
        .map_err(|err| err.to_string())?
        .json()
        .await
        .map_err(|err| err.to_string())
}

pub async fn save_run(run: &RunDto) -> Result<SavedRunDto, String> {
    Request::post("/api/runs")
        .json(run)
        .map_err(|err| err.to_string())?
        .send()
        .await
        .map_err(|err| err.to_string())?
        .json()
        .await
        .map_err(|err| err.to_string())
}

pub async fn run_detail(id: i64) -> Result<RunDto, String> {
    Request::get(&format!("/api/runs/{id}"))
        .send()
        .await
        .map_err(|err| err.to_string())?
        .json()
        .await
        .map_err(|err| err.to_string())
}

pub async fn leaderboard() -> Result<Vec<LeaderboardRow>, String> {
    Request::get("/api/leaderboard?limit=50")
        .send()
        .await
        .map_err(|err| err.to_string())?
        .json()
        .await
        .map_err(|err| err.to_string())
}

fn level_param(level: Level) -> &'static str {
    match level {
        Level::ATP250 => "ATP250",
        Level::ATP500 => "ATP500",
        Level::ATP1000 => "ATP1000",
        Level::GrandSlam => "GrandSlam",
    }
}

fn encode(value: &str) -> String {
    value.replace(' ', "%20")
}
