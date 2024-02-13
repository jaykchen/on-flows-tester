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

use reqwest::{
    header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE},
    Client,
};
use serde::{Deserialize, Serialize};

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
    let query =
        "<s> Below is an instruction that describes a task, paired with an input that provides further context. Write a response that appropriately completes the request.\n\n### Instruction:\nYou're a programming bot tasked to analyze GitHub issues data and assign labels to them.\n\n### Input:\nCan you assign labels to the GitHub issue titled `feat: Implement typed function references proposal`, created by `hydai`, stating `This issue pertains to the Typed function references proposal for WebAssembly, aiming to enhance function references for efficient indirect calls and better interoperability. The key goals include enabling direct function calls without runtime checks, representing function pointers without using tables, and facilitating the exchange of function references between modules and the host environment. Additionally, the proposal seeks to support safe closures and separate useful features from the GC proposal.\n\nTo address the requirements, the following tasks need to be completed:\n- Gain familiarity with the Wasm Spec\n- Study the Typed function references Spec\n- Integrate new type definitions and instructions in WasmEdge\n- Implement an option in WasmEdge CLI to enable/disable the proposal\n- Develop unit tests for comprehensive coverage\n\nTo qualify for the LFX mentorship, the applicant should have experience in C++ programming and complete challenge #1221.\n\nReferences:\n- GC Proposal: [WebAssembly GC](https://github.com/WebAssembly/gc)\n- Typed function references Proposal: [Function References](https://github.com/WebAssembly/function-references)`?\n\n### Response:";

    match completion_inner_async(query).await {
        Ok(res) => log::info!("res: {:?}", res),

        Err(_e) => log::error!("error: {:?}", _e),
    }

    Ok(())
}

pub async fn completion_inner_async(user_input: &str) -> anyhow::Result<String> {
    let llm_endpoint = "https://api-inference.huggingface.co/models/jaykchen/tiny".to_string();
    let llm_api_key = env::var("LLM_API_KEY").expect("LLM_API_KEY-must-be-set");

    let client = Client::new();

    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {}", llm_api_key)).unwrap(),
    );

    let body = serde_json::json!({
        "inputs": user_input,
    });

    use anyhow::Context;

    let response = client
        .post(llm_endpoint)
        .headers(headers)
        .json(&body)
        .send()
        .await
        .context("Failed to send request to API")?; // Adds context to the error

    let status_code = response.status();

    if status_code.is_success() {
        let response_body = response
            .text()
            .await
            .context("Failed to read response body")?;

        let completion_response: Vec<Choice> =
            serde_json::from_str(&response_body).context("Failed to parse response from API")?;

        if let Some(choice) = completion_response.get(0) {
            log::info!("choice: {:?}", choice);
            Ok(choice.generated_text.clone())
        } else {
            Err(anyhow::anyhow!(
                "No completion choices found in the response"
            ))
        }
    } else {
        Err(anyhow::anyhow!(
            "Failed to get a successful response from the API"
        ))
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Choice {
    pub generated_text: String,
}

