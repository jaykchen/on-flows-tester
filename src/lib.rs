use airtable_flows::create_record;
use anyhow;
use chrono::{DateTime, Datelike, Duration, NaiveDate, Timelike, Utc};
use dotenv::dotenv;
use flowsnet_platform_sdk::logger;
use github_flows::{get_octo, octocrab, GithubLogin};
use schedule_flows::{schedule_cron_job, schedule_handler};
use serde::{Deserialize, Serialize};
use serde_json;
use std::{collections::HashSet, env};

#[no_mangle]
#[tokio::main(flavor = "current_thread")]
pub async fn on_deploy() {
    let now = Utc::now();
    let now_minute = now.minute() + 1;
    let cron_time = format!(
        "{:02} {:02} {:02} {:02} *",
        now_minute,
        now.hour(),
        now.day(),
        now.month(),
    );
    schedule_cron_job(cron_time, String::from("cron_job_evoked")).await;
}

#[schedule_handler]
async fn handler(body: Vec<u8>) {
    dotenv().ok();
    logger::init();
    let owner = env::var("owner").unwrap_or("wasmedge".to_string());
    let repo = env::var("repo").unwrap_or("wasmedge".to_string());

    let now = Utc::now();
    let n_days_ago = (now - Duration::days(7)).date_naive();

    if let Ok(repo_data) = is_valid_owner_repo_integrated(&owner, &repo).await {
        log::info!("user_vec.len(): {:?}", repo_data);
    }
}

pub async fn upload_airtable(login: &str, email: &str, twitter_username: &str, watching: bool) {
    let airtable_token_name = env::var("airtable_token_name").unwrap_or("github".to_string());
    let airtable_base_id = env::var("airtable_base_id").unwrap_or("appmhvMGsMRPmuUWJ".to_string());
    let airtable_table_name = env::var("airtable_table_name").unwrap_or("mention".to_string());

    let data = serde_json::json!({
        "Name": login,
        "Email": email,
        "Twitter": twitter_username,
        "Watching": watching,
    });
    let _ = create_record(
        &airtable_token_name,
        &airtable_base_id,
        &airtable_table_name,
        data.clone(),
    );
}

async fn get_user_data(login: &str) -> anyhow::Result<(String, String)> {
    #[derive(Serialize, Deserialize, Debug)]
    struct UserProfile {
        login: String,
        company: Option<String>,
        location: Option<String>,
        email: Option<String>,
        twitter_username: Option<String>,
    }

    let octocrab = get_octo(&GithubLogin::Default);
    let user_profile_route = format!("users/{}", login);

    match octocrab
        .get::<UserProfile, _, ()>(&user_profile_route, None::<&()>)
        .await
    {
        Ok(profile) => {
            let email = profile.email.unwrap_or("no email".to_string());
            let twitter_username = profile.twitter_username.unwrap_or("no twitter".to_string());

            Ok((email, twitter_username))
        }
        Err(e) => {
            log::error!("Failed to get user info for {}: {:?}", login, e);
            Err(e.into())
        }
    }
}

pub async fn is_valid_owner_repo_integrated(owner: &str, repo: &str) -> anyhow::Result<String> {
    #[derive(Deserialize)]
    struct CommunityProfile {
        health_percentage: u16,
        description: Option<String>,
        readme: Option<FileDetails>,
        updated_at: Option<DateTime<Utc>>,
    }
    #[derive(Debug, Deserialize, Serialize)]
    pub struct FileDetails {
        url: Option<String>,
        html_url: Option<String>,
    }
    let community_profile_url = format!("/repos/{}/{}/community/profile", owner, repo);

    let mut description = String::new();
    let mut date = Utc::now().date_naive();
    let octocrab = get_octo(&GithubLogin::Default);

    match octocrab
        .get::<CommunityProfile, _, ()>(&community_profile_url, None::<&()>)
        .await
    {
        Ok(profile) => {
            description = profile
                .description
                .as_ref()
                .unwrap_or(&String::from(""))
                .to_string();
            date = profile
                .updated_at
                .as_ref()
                .unwrap_or(&Utc::now())
                .date_naive();
        }
        Err(e) => log::error!("Error parsing Community Profile: {:?}", e),
    }

    Ok(format!("{}/{}", description, date))
}

/* query ($org: String!, $repoName: String!, $since: GitTimestamp!, $afterCursor: String) {
  organization(login: $org) {
    repository(name: $repoName) {
      id
      name
      url
      defaultBranchRef {
        name
        target {
          ... on Commit {
            id
            history(first: 100, since: $since, after: $afterCursor) {
              pageInfo {
                hasNextPage
                endCursor
              }
              edges {
                node {
                  author {
                    user {
                      login
                      name
                    }
                  }
                  committer {
                    user {
                      login
                      name
                    }
                  }
                }
              }
            }
          }
        }
      }
    }
  }
} */
