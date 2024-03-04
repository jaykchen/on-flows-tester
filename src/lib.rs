pub mod llm_service;
use chrono::{Datelike, Timelike, Utc};
use dotenv::dotenv;
use flowsnet_platform_sdk::logger;
use github_flows::{get_octo, GithubLogin};
use octocrab_wasi::{issues, params::issues::Sort, params::Direction};
use openai_flows::{
    chat::{ChatModel, ChatOptions},
    OpenAIFlows,
};
use schedule_flows::{schedule_cron_job, schedule_handler};
use serde_json::{json, to_string_pretty, Value};
use std::{collections::HashMap, env};

use http_req::{
    request::{Method, Request},
    response::Response,
    uri::Uri,
};
use serde::{Deserialize, Serialize};
use chrono::Duration;

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

    let _ = search_for_mention().await;
}
async fn search_for_mention() -> anyhow::Result<()> {
    let octocrab = get_octo(&GithubLogin::Default);
    let one_hour_ago = (Utc::now() - Duration::hours(100i64))
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();
    let one_year_ago = (Utc::now() - Duration::weeks(52i64))
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();

    let query = format!("is:issue mentions:Hacktoberfest updated:>{one_year_ago}");
    log::error!("query: {:?}", query.clone());

    let issues = octocrab
        .search()
        .issues_and_pull_requests(&query)
        .sort("comments")
        .order("desc")
        .send()
        .await?;

    for issue in issues.items {
        log::error!("issue: {:?}", issue.title);
    }

    Ok(())
}

pub async fn completion_inner_async(user_input: &str) -> anyhow::Result<String> {
    let llm_endpoint = "https://api-inference.huggingface.co/models/jaykchen/tiny";
    let llm_api_key = env::var("LLM_API_KEY").expect("LLM_API_KEY-must-be-set");
    let base_url = Uri::try_from(llm_endpoint).expect("Failed to parse URL");

    let mut writer = Vec::new(); // This will store the response body

    let query = json!({
        "inputs": user_input,
        "wait_for_model": true,
        "max_new_tokens": 500,
    });
    let query_bytes = serde_json::to_vec(&query).expect("Failed to serialize query to bytes");

    // let query_str = query.to_string();
    let query_len = query_bytes.len().to_string();
    // Prepare and send the HTTP request
    match Request::new(&base_url)
        .method(Method::POST)
        .header("Content-Type", "application/json")
        .header("Authorization", &format!("Bearer {}", llm_api_key))
        .header("Content-Length", &query_len)
        .body(&query_bytes)
        .send(&mut writer)
    {
        Ok(res) => {
            if !res.status_code().is_success() {
                log::error!("HTTP error with status {:?}", res.status_code());
                return Err(anyhow::anyhow!(
                    "HTTP error with status {:?}",
                    res.status_code()
                ));
            }

            // Attempt to parse the response body into the expected structure
            let completion_response: Vec<Choice> =
                serde_json::from_slice(&writer).expect("Failed to parse response from API");

            if let Some(choice) = completion_response.get(0) {
                log::info!("Choice: {:?}", choice);
                Ok(choice.generated_text.clone())
            } else {
                Err(anyhow::anyhow!(
                    "No completion choices found in the response"
                ))
            }
        }
        Err(e) => {
            log::error!("Error getting response from API: {:?}", e);

            Err(anyhow::anyhow!("Error getting response from API: {:?}", e))
        }
    }
}
#[derive(Serialize, Deserialize, Debug)]
pub struct Choice {
    pub generated_text: String,
}

async fn get_watchers(owner_repo: &str) -> anyhow::Result<HashMap<String, (String, String)>> {
    #[derive(Serialize, Deserialize, Debug)]
    struct GraphQLResponse {
        data: Option<RepositoryData>,
    }

    #[derive(Serialize, Deserialize, Debug)]
    struct RepositoryData {
        repository: Option<Repository>,
    }

    #[derive(Serialize, Deserialize, Debug)]
    struct Repository {
        watchers: Option<WatchersConnection>,
    }

    #[derive(Serialize, Deserialize, Debug)]
    struct WatcherEdge {
        node: Option<WatcherNode>,
    }
    #[derive(Serialize, Deserialize, Debug)]
    struct WatcherNode {
        login: String,
        email: Option<String>,
        twitterUsername: Option<String>,
    }

    #[derive(Serialize, Deserialize, Debug)]
    struct PageInfo {
        #[serde(rename = "endCursor")]
        end_cursor: Option<String>,
        #[serde(rename = "hasNextPage")]
        has_next_page: Option<bool>,
    }

    #[derive(Serialize, Deserialize, Debug)]
    struct WatchersConnection {
        edges: Option<Vec<WatcherEdge>>,
        #[serde(rename = "pageInfo")]
        page_info: Option<PageInfo>,
    }
    let mut watchers_map = HashMap::<String, (String, String)>::new();

    let mut after_cursor = None;
    let (owner, repo) = owner_repo.split_once("/").unwrap_or_default();

    for _n in 1..499 {
        let query_str = format!(
            r#"
            query {{
                repository(owner: "{}", name: "{}") {{
                    watchers(first: 100, after: {}) {{
                        edges {{
                            node {{
                                login
                                email
                                twitterUsername
                            }}
                        }}
                        pageInfo {{
                            endCursor
                            hasNextPage
                        }}
                    }}
                }}
            }}
            "#,
            owner,
            repo,
            after_cursor
                .as_ref()
                .map_or("null".to_string(), |c| format!(r#""{}""#, c))
        );

        let response: GraphQLResponse;
        match github_http_post_gql(&query_str).await {
            Ok(r) => {
                response = match serde_json::from_slice::<GraphQLResponse>(&r) {
                    Ok(res) => res,
                    Err(err) => {
                        log::error!("Failed to deserialize response from Github: {}", err);
                        continue;
                    }
                };
            }
            Err(_e) => {
                continue;
            }
        }
        let watchers = response
            .data
            .and_then(|data| data.repository)
            .and_then(|repo| repo.watchers);

        if let Some(watchers) = watchers {
            for edge in watchers.edges.unwrap_or_default() {
                if let Some(node) = edge.node {
                    watchers_map.insert(
                        node.login,
                        (
                            node.email.unwrap_or(String::from("")),
                            node.twitterUsername.unwrap_or(String::from("")),
                        ),
                    );
                }
            }

            match watchers.page_info {
                Some(page_info) if page_info.has_next_page.unwrap_or(false) => {
                    after_cursor = page_info.end_cursor;
                }
                _ => {
                    log::info!("watchers loop {}", _n);
                    break;
                }
            }
        } else {
            break;
        }
    }
    Ok(watchers_map)
}
pub async fn github_http_post_gql(query: &str) -> anyhow::Result<Vec<u8>> {
    use http_req::{request::Method, request::Request, uri::Uri};
    let token = env::var("GITHUB_TOKEN").expect("github_token is required");
    let base_url = "https://api.github.com/graphql";
    let base_url = Uri::try_from(base_url).unwrap();
    let mut writer = Vec::new();

    let query = serde_json::json!({"query": query});
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
