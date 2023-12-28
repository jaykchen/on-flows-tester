use airtable_flows::create_record;
use anyhow;
use chrono::{DateTime, Datelike, Duration, NaiveDate, Timelike, Utc};
use derivative::Derivative;
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
    let cron_time = format!("{:02} {:02} {:02} * *", now_minute, now.hour(), now.day());
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
    match get_commits_in_range(&owner, &repo, Some(String::from("hydai")), 7u16, None).await {
        Some((size, my_vec, team_vec)) => {
            log::info!("count: {:?}, my-vec: {:?}, team-vec: {:?}", size, my_vec, team_vec);
        }
        None => {
            log::error!("Failed to get commits in range");
        }
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

pub async fn get_commits_in_range(
    owner: &str,
    repo: &str,
    user_name: Option<String>,
    range: u16,
    token: Option<String>,
) -> Option<(usize, Vec<GitMemory>, Vec<GitMemory>)> {
    #[derive(Debug, Deserialize, Serialize, Clone)]
    struct User {
        login: String,
    }

    #[derive(Serialize, Deserialize, Debug)]
    struct GithubCommit {
        sha: String,
        html_url: String,
        author: Option<User>,    // made nullable
        committer: Option<User>, // made nullable
        commit: CommitDetails,
    }

    #[derive(Serialize, Deserialize, Debug)]
    struct CommitDetails {
        author: CommitUserDetails,
        message: String,
        // committer: CommitUserDetails,
    }

    #[derive(Serialize, Deserialize, Debug)]
    struct CommitUserDetails {
        date: Option<DateTime<Utc>>,
    }
    let token_str = match &token {
        None => String::from(""),
        Some(t) => format!("&token={}", t.as_str()),
    };
    let base_commit_url = format!("repos/{owner}/{repo}/commits?&per_page=100{token_str}");
    // let base_commit_url =
    //     format!("https://api.github.com/repos/{owner}/{repo}/commits?&per_page=100{token_str}");

    let mut git_memory_vec = vec![];
    let mut weekly_git_memory_vec = vec![];
    let now = Utc::now();
    let n_days_ago = (now - Duration::days(range as i64)).date_naive();
    let octocrab = get_octo(&GithubLogin::Default);

    match octocrab
        .get::<Vec<GithubCommit>, _, ()>(&base_commit_url, None::<&()>)
        .await
    {
        Err(e) => {
            log::error!("Error parsing commits: {:?}", e);
        }
        Ok(commits) => {
            for commit in commits {
                if let Some(commit_date) = &commit.commit.author.date {
                    if commit_date.date_naive() <= n_days_ago {
                        continue;
                    }
                    weekly_git_memory_vec.push(GitMemory {
                        memory_type: MemoryType::Commit,
                        name: commit.author.clone().map_or(String::new(), |au| au.login),
                        tag_line: commit.commit.message.clone(),
                        source_url: commit.html_url.clone(),
                        payload: String::from(""),
                        date: commit_date.date_naive(),
                    });
                    if let Some(user_name) = &user_name {
                        if let Some(author) = &commit.author {
                            if author.login.as_str() == user_name {
                                git_memory_vec.push(GitMemory {
                                    memory_type: MemoryType::Commit,
                                    name: author.login.clone(),
                                    tag_line: commit.commit.message.clone(),
                                    source_url: commit.html_url.clone(),
                                    payload: String::from(""),
                                    date: commit_date.date_naive(),
                                });
                            }
                        }
                    }
                }
            }
        }
    }
    if user_name.is_none() {
        git_memory_vec = weekly_git_memory_vec.clone();
    }
    let count = git_memory_vec.len();
    Some((count, git_memory_vec, weekly_git_memory_vec))
}

pub async fn get_readme(owner: &str, repo: &str) -> Option<String> {
    #[derive(Deserialize, Debug)]
    struct GithubReadme {
        content: Option<String>,
    }

    let readme_url = format!("repos/{owner}/{repo}/readme");

    let octocrab = get_octo(&GithubLogin::Default);

    match octocrab
        .get::<GithubReadme, _, ()>(&readme_url, None::<&()>)
        .await
    {
        Ok(readme) => {
            if let Some(c) = readme.content {
                let cleaned_content = c.replace("\n", "");
                match base64::decode(&cleaned_content) {
                    Ok(decoded_content) => match String::from_utf8(decoded_content) {
                        Ok(out) => {
                            return Some(format!("Readme: {}", out));
                        }
                        Err(e) => {
                            log::error!("Failed to convert cleaned readme to String: {:?}", e);
                            return None;
                        }
                    },
                    Err(e) => {
                        log::error!("Error decoding base64 content: {:?}", e);
                        None
                    }
                }
            } else {
                log::error!("Content field in readme is null.");
                None
            }
        }
        Err(e) => {
            log::error!("Error parsing Readme: {:?}", e);
            None
        }
    }
}

#[derive(Derivative, Serialize, Deserialize, Debug, Clone)]
pub struct GitMemory {
    pub memory_type: MemoryType,
    #[derivative(Default(value = "String::from(\"\")"))]
    pub name: String,
    #[derivative(Default(value = "String::from(\"\")"))]
    pub tag_line: String,
    #[derivative(Default(value = "String::from(\"\")"))]
    pub source_url: String,
    #[derivative(Default(value = "String::from(\"\")"))]
    pub payload: String,
    pub date: NaiveDate,
}
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum MemoryType {
    Commit,
    Issue,
    Discussion,
    Meta,
}
