use anyhow;
use async_openai::{
    types::{
        // ChatCompletionFunctionsArgs, ChatCompletionRequestMessage,
        ChatCompletionRequestSystemMessageArgs,
        ChatCompletionRequestUserMessageArgs,
        // ChatCompletionTool, ChatCompletionToolArgs, ChatCompletionToolType,
        CreateChatCompletionRequestArgs,
        FinishReason,
    },
    Client,
};
use chrono::{DateTime, Datelike, Duration, NaiveDate, Timelike, Utc};
use derivative::Derivative;
use dotenv::dotenv;
use flowsnet_platform_sdk::logger;
use github_flows::{
    get_octo,
    octocrab::{self, Page},
    GithubLogin,
};
use schedule_flows::{schedule_cron_job, schedule_handler};
use serde::{Deserialize, Serialize};
use serde_json;
use std::{collections::HashSet, env};

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
    let OPENAI_API_KEY = env::var("OPENAI_API_KEY").unwrap_or("".to_string());

    let owner = env::var("owner").unwrap_or("wasmedge".to_string());
    let repo = env::var("repo").unwrap_or("wasmedge".to_string());

    let now = Utc::now();
    let n_days_ago = (now - Duration::days(7)).date_naive();

    use tokio::time::Instant;
    let start_time = Instant::now();

    if let Ok((sys, usr)) = get_commit("jaykchen", "what", "https://github.com/jaykchen/github-analyzer-2/commit/61c9f5f6b3b454eff0481746504d7112d441b578.patch", false, false, None).await {
        let elapsed = start_time.elapsed();
        log::info!(
            "Time elapsed in aggregate_commits  is: {} seconds",elapsed.as_secs());
    
        log::info!("sys: {:?}, user: {:?}", sys, usr);
    }
}

// if let Ok(commit) = analyze_commit_integrated().await {
//     {
//         log::info!("summary: {:?}", commit);
//     }
// }
/*     if let Some(commit) = analyze_commit_integrated(&owner, &repo).await {
       if let Ok(summary) = chain_of_chat(
           "Go through the document and extract key information ",
           &format!("Document: {}", readme),
           "Step-1",
           1000,
           "Create a concise summary based on the key information extracted from the document",
           300,
           "Failed to get reply from OpenAI",
       ).await {
           log::info!("summary: {:?}", summary);
       }
   }
*/

pub async fn get_commit(
    user_name: &str,
    tag_line: &str,
    url: &str,
    _turbo: bool,
    is_sparce: bool,
    token: Option<String>,
) -> anyhow::Result<(String, String)> {
    use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE, USER_AGENT};

    let token_str = match token {
        None => String::new(),
        Some(ref t) => format!("&token={}", t.as_str()),
    };

    // let commit_patch_str = format!("{url}.patch{token_str}");

    let github_token = std::env::var("GITHUB_TOKEN").unwrap();

    let mut headers = HeaderMap::new();
    headers.insert(
        USER_AGENT,
        HeaderValue::from_static("flows-network connector"),
    );
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("plain/text"));
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {}", github_token))?,
    );

    let client = reqwest::Client::new();
    let response = client.get(url).headers(headers).send().await?;

    if !response.status().is_success() {
        log::error!("GitHub HTTP error: {}", response.status());
        return Err(anyhow::anyhow!("GitHub HTTP error: {}", response.status()));
    }

    let text = response.text().await?;

    let sys_prompt_1 = &format!(
        "Given a commit patch from user {user_name}, analyze its content. Focus on changes that substantively alter code or functionality. A good analysis prioritizes the commit message for clues on intent and refrains from overstating the impact of minor changes. Aim to provide a balanced, fact-based representation that distinguishes between major and minor contributions to the project. Keep your analysis concise."
    );

    let stripped_texts = text;

    let usr_prompt_1 = &format!(
        "Analyze the commit patch: {stripped_texts}, and its description: {tag_line}. Summarize the main changes, but only emphasize modifications that directly affect core functionality. A good summary is fact-based, derived primarily from the commit message, and avoids over-interpretation. It recognizes the difference between minor textual changes and substantial code adjustments. Conclude by evaluating the realistic impact of {user_name}'s contributions in this commit on the project. Limit the response to 110 tokens."
    );

    return Ok((sys_prompt_1.to_string(), usr_prompt_1.to_string()));
}

pub async fn chain_of_chat(
    sys_prompt_1: &str,
    usr_prompt_1: &str,
    chat_id: &str,
    gen_len_1: u16,
    usr_prompt_2: &str,
    gen_len_2: u16,
    error_tag: &str,
) -> anyhow::Result<String> {
    let client = Client::new();

    let mut messages = vec![
        ChatCompletionRequestSystemMessageArgs::default()
            .content(sys_prompt_1)
            .build()
            .expect("Failed to build system message")
            .into(),
        ChatCompletionRequestUserMessageArgs::default()
            .content(usr_prompt_1)
            .build()?
            .into(),
    ];
    let request = CreateChatCompletionRequestArgs::default()
        .max_tokens(gen_len_1)
        .model("gpt-3.5-turbo-1106")
        .messages(messages.clone())
        .build()?;

    let chat = client.chat().create(request).await?;
    if let Some(text) = chat.choices[0].message.clone().content {
        log::info!("Step 1: {:?}", text);
    }
    messages.push(
        ChatCompletionRequestUserMessageArgs::default()
            .content(usr_prompt_2)
            .build()?
            .into(),
    );

    let request = CreateChatCompletionRequestArgs::default()
        .max_tokens(gen_len_2)
        .model("gpt-3.5-turbo-1106")
        .messages(messages)
        .build()?;

    let chat = client.chat().create(request).await?;
    log::error!("Complete Msg: {:?}", chat.choices[0].message.clone());

    match chat.choices[0].message.clone().content {
        Some(res) => {
            log::info!("Step 2: {:?}", res);
            Ok(res)
        }
        None => return Err(anyhow::anyhow!(error_tag.to_string())),
    }
}

pub async fn chat_inner(
    system_prompt: &str,
    user_input: &str,
    max_token: u16,
    model: &str,
) -> anyhow::Result<String> {
    let client = Client::new();

    let messages = vec![
        ChatCompletionRequestSystemMessageArgs::default()
            .content(system_prompt)
            .build()
            .expect("Failed to build system message")
            .into(),
        ChatCompletionRequestUserMessageArgs::default()
            .content(user_input)
            .build()?
            .into(),
    ];
    let request = CreateChatCompletionRequestArgs::default()
        .max_tokens(max_token)
        .model(model)
        .messages(messages)
        .build()?;

    let chat = client.chat().create(request).await?;

    // let check = chat.choices.get(0).clone().unwrap();
    // send_message_to_channel("ik8", "general", format!("{:?}", check)).await;

    match chat.choices[0].message.clone().content {
        Some(res) => {
            log::info!("{:?}", chat.choices[0].message.clone());
            Ok(res)
        }
        None => Err(anyhow::anyhow!("Failed to get reply from OpenAI")),
    }
}


