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
// pub async fn chain_of_chat(
//     sys_prompt_1: &str,
//     usr_prompt_1: &str,
//     chat_id: &str,
//     gen_len_1: u16,
//     usr_prompt_2: &str,
//     gen_len_2: u16,
//     error_tag: &str,
// ) -> anyhow::Result<String> {

#[schedule_handler]
async fn handler(body: Vec<u8>) {
    dotenv().ok();
    logger::init();
    let OPENAI_API_KEY = env::var("OPENAI_API_KEY").unwrap_or("".to_string());

    let owner = env::var("owner").unwrap_or("wasmedge".to_string());
    let repo = env::var("repo").unwrap_or("wasmedge".to_string());

    let now = Utc::now();
    let n_days_ago = (now - Duration::days(7)).date_naive();

    if let Ok(commit) = analyze_commit_integrated().await {
        {
            log::info!("summary: {:?}", commit);
        }
    }
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
}

pub async fn analyze_commit_integrated() -> anyhow::Result<String> {
    // let commit_patch_str = format!("{url}.patch{token_str}");
    let commit_patch_str = format!("https://github.com/WasmEdge/WasmEdge/commit/c62e0bb3056bea6d26dab0e626de179cf0616243.patch");
    let octocrab = get_octo(&GithubLogin::Default);
    let user_name = "hydai";
    let response = octocrab._get(&commit_patch_str, None::<&()>).await?;
    let text: String = response.text().await?;

    let sys_prompt_1 = &format!(
                "Given a commit patch from user {user_name}, analyze its content. Focus on changes that substantively alter code or functionality. A good analysis prioritizes the commit message for clues on intent and refrains from overstating the impact of minor changes. Aim to provide a balanced, fact-based representation that distinguishes between major and minor contributions to the project. Keep your analysis concise."
            );

    // let stripped_texts = if !is_sparce {
    //     let stripped_texts = text
    //         .splitn(2, "diff --git")
    //         .nth(0)
    //         .unwrap_or("")
    //         .to_string();

    //     let stripped_texts = squeeze_fit_remove_quoted(&stripped_texts, 5_000, 1.0);
    //     squeeze_fit_post_texts(&stripped_texts, 3_000, 0.6)
    // } else {
    //     text.chars().take(24_000).collect::<String>()
    // };
    let stripped_texts = text;
    let tag_line = "";

    let usr_prompt_1 = &format!(
                "Analyze the commit patch: {stripped_texts}, and its description: {tag_line}. Summarize the main changes, but only emphasize modifications that directly affect core functionality. A good summary is fact-based, derived primarily from the commit message, and avoids over-interpretation. It recognizes the difference between minor textual changes and substantial code adjustments. Conclude by evaluating the realistic impact of {user_name}'s contributions in this commit on the project. Limit the response to 110 tokens."
            );

    match chat_inner(sys_prompt_1, usr_prompt_1, 128, "gpt-3.5-turbo-1106").await {
        Ok(r) => {
            let out = format!(" {}", r);
            Ok(out)
        }
        Err(_e) => {
            log::error!("Error generating issue summary : {}", _e);
            Err(anyhow::anyhow!("Error generating issue summary : {}", _e))
        }
    }
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

    match chat.choices[0].message.clone().content {
        Some(res) => {
            log::info!("{:?}", res);
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
