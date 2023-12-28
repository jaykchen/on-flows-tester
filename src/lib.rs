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

    if let Some(readme) = get_readme(&owner, &repo).await {
        if let Ok(summary) = chat_inner(
            "Go through the document and extract key information ",
            &format!("Document: {}", readme),
            300,
            "gpt-3.5-turbo-1106",
        ).await {
            log::info!("summary: {:?}", summary);
        }
    }

    // if let Some(readme) = get_readme(&owner, &repo).await {
    //     if let Ok(summary) = chain_of_chat(
    //         "Go through the document and extract key information ",
    //         &format!("Document: {}", readme),
    //         "Step-1",
    //         1000,
    //         "Create a concise summary based on the key information extracted from the document",
    //         300,
    //         "Failed to get reply from OpenAI",
    //     ).await {
    //         log::info!("summary: {:?}", summary);
    //     }
    // }
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

    match chat.choices[0].message.clone().content {
        Some(res) => {
            log::info!("{:?}", res);
        }
        None => return Err(anyhow::anyhow!(error_tag.to_string())),
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
        Some(res) => {log::info!("{:?}", chat.choices[0].message.clone()); Ok(res)},
        None => Err(anyhow::anyhow!("Failed to get reply from OpenAI")),
    }
}
