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
    let octocrab = get_octo(&GithubLogin::Default);
    let report_issue_handle = octocrab.issues("pytorch", "pytorch");

    let report_issue = report_issue_handle
        .update(1329)
        .body("hard coded demo")
        .send()
        .await?;
    // let report_issue_number = report_issue.number;
    // let label_slice = vec!["fake".to_string()];
    // let _ = report_issue_handle.update(report_issue_number).labels(&label_slice).send().await?;

    Ok(())
}
