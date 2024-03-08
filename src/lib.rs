use chrono::{Datelike, Timelike, Utc};
use dotenv::dotenv;
use flowsnet_platform_sdk::logger;
use github_flows::{get_octo, GithubLogin};
use octocrab_wasi::{issues, params::issues::Sort, params::Direction};
use schedule_flows::{schedule_cron_job, schedule_handler};
use serde_json::{json, to_string_pretty, Value};

#[no_mangle]
#[tokio::main(flavor = "current_thread")]
pub async fn on_deploy() {
    let now = Utc::now();
    let now_minute = now.minute() + 2;
    let cron_time = format!("{:02} {:02} {:02} * *", now_minute, now.hour(), now.day());
    schedule_cron_job(cron_time, String::from("cron_job_evoked")).await;
}

#[schedule_handler]
async fn handler(body: Vec<u8>) {
    dotenv().ok();
    logger::init();

    let _ = inner().await;
}
async fn inner() -> anyhow::Result<()> {
    let _ = github_http_post_issue_comment("hard coded comment by flows").await?;
    // let octocrab = get_octo(&GithubLogin::Default);
    // let report_issue_handle = octocrab.issues("pytorch", "pytorch");

    // let report_issue = report_issue_handle.create_comment(1329, "hard coded comment by flows").await?;
    // let report_issue_number = report_issue.number;
    // let label_slice = vec!["fake".to_string()];
    // let _ = report_issue_handle.update(report_issue_number).labels(&label_slice).send().await?;

    Ok(())
}
pub async fn github_http_post_issue_comment(query: &str) -> anyhow::Result<Vec<u8>> {
    dotenv().ok();

    use http_req::{request::Method, request::Request, uri::Uri};
    let token = std::env::var("GITHUB_TOKEN").expect("github_token is required");

    // let base_url = "https://api.github.com/repos/{owner}/{repo}/issues/{issue}/comments";
    let base_url = "https://api.github.com/repos/pytorch/pytorch/issues/1329/comments";

    let base_url = Uri::try_from(base_url).unwrap();
    let mut writer = Vec::new();

    let query = serde_json::json!({"body": query});
    match Request::new(&base_url)
        .method(Method::POST)
        .header("User-Agent", "flows-network connector")
        .header("Content-Type", "application/json")
        .header("Authorization", &format!("Bearer {}", token))
        .header("Content-Length", &query.to_string().len())
        .body(&query.to_string().into_bytes())
        .send(&mut writer)
    {
        Ok(res) => {
            if !res.status_code().is_success() {
                log::error!("Github http error {:?}", res.status_code());
                return Err(anyhow::anyhow!("Github http error {:?}", res.status_code()));
            }
            Ok(writer)
        }
        Err(_e) => {
            log::error!("Error getting response from Github: {:?}", _e);
            Err(anyhow::anyhow!(_e))
        }
    }
}
