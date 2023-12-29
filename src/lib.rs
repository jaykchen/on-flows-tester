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

    if let Ok(commit) = analyze_commit_integrated().await {

        if let Ok(sum) =
            chat_inner("you're a language bot", &commit, 300, "gpt-3.5-turbo-1106").await
        {
            log::info!("summary: {:?}", sum);
        }
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

pub async fn analyze_commit_integrated() -> anyhow::Result<String> {
    // let commit_patch_str = format!("{url}.patch{token_str}");
    let commit_patch_str = "https://github.com/WasmEdge/WasmEdge/commit/c62e0bb3056bea6d26dab0e626de179cf0616243.patch";
    // let octocrab = get_octo(&GithubLogin::Default);
    let user_name = "hydai";

    let octocrab = get_octo(&GithubLogin::Default);

    let response = octocrab._get(commit_patch_str, None::<&()>).await;

    let mut text = String::new();
    
    match response {
        Ok(resp) => {
            if resp.status().is_success() {
                match resp.text().await {
                    Ok(t) => {
                        log::info!("Successfully fetched the commit patch: {}.", t.clone());
                        text = t;
                    }
                    Err(e) => {
                        log::error!("Failed to read response text: {}", e);
                    }
                }
            } else {
                log::error!("Request failed with status: {}", resp.status());
            }
        }
        Err(e) => {
            log::error!("Request failed: {}", e);
        }
    }


    // let text = match response {
    //     Ok(r) => {
    //         log::info!("text parsed from Response: {:?}", r);
    //         r
    //     }
    //     Err(e) => {
    //         log::info!("Failed to get the commit patch: {}", e);
    //         return Err(anyhow::anyhow!("Failed to get the commit patch"));
    //     }
    // };

    // log::info!("commit: {:?}", &response.items[0]);
    // let sys_prompt_1 = &format!(
    //             "Given a commit patch from user {user_name}, analyze its content. Focus on changes that substantively alter code or functionality. A good analysis prioritizes the commit message for clues on intent and refrains from overstating the impact of minor changes. Aim to provide a balanced, fact-based representation that distinguishes between major and minor contributions to the project. Keep your analysis concise."
    //         );

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

    let sys_prompt_1 =
        &format!("You're an AI bot that specializes in analyzing Github documents and behaviours");
    let usr_prompt_1 = &format!(
                "Analyze the commit patch: {stripped_texts:?}, and its description: {tag_line}. List the main changes, only emphasize modifications that directly affect core functionality. Please recognize the difference between minor textual changes and substantial code adjustments."
            );

    let usr_prompt_2 = &format!(
                "Given the main changes you've identified, make a concise summary based on them, only emphasize modifications that directly affect core functionality. A good summary is fact-based, derived primarily from the commit message, and avoids over-interpretation. It recognizes the difference between minor textual changes and substantial code adjustments. Conclude by evaluating the realistic impact of {user_name}'s contributions in this commit on the project. Limit the response to 110 tokens."
            );

    chain_of_chat(
        sys_prompt_1,
        usr_prompt_1,
        "this_chat",
        800,
        usr_prompt_2,
        128,
        "chained_chats",
    )
    .await
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
