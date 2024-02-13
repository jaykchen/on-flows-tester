pub mod llm_service;
use chrono::{ Datelike, Timelike, Utc };
use dotenv::dotenv;
use flowsnet_platform_sdk::logger;
use github_flows::{ get_octo, GithubLogin };
use octocrab_wasi::{ issues, params::issues::Sort, params::Direction };
use openai_flows::{ chat::{ ChatModel, ChatOptions }, OpenAIFlows };
use schedule_flows::{ schedule_cron_job, schedule_handler };
use serde_json::{ json, to_string_pretty, Value };
use std::{ collections::HashMap, env };

use serde::{ Deserialize, Serialize };
use http_req::{ request::{ Request, Method }, response::Response, uri::Uri };

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
    let report_issue_handle = octocrab.issues("jaykchen", "issue-labeler");

    let report_issue = report_issue_handle
        .create("hardcoded".to_string())
        .body("demo")
        .labels(Some(vec!["hardcoded".to_string()]))
        .send().await?;
    let report_issue_number = report_issue.number;
    let label_slice = vec!["fake".to_string()];
    let _ = report_issue_handle.update(report_issue_number).labels(&label_slice).send().await?;

    let query =
        "<s> Below is an instruction that describes a task, paired with an input that provides further context. Write a response that appropriately completes the request.\n\n### Instruction:\nYou're a programming bot tasked to analyze GitHub issues data and assign labels to them.\n\n### Input:\nCan you assign labels to the GitHub issue titled `feat: Implement typed function references proposal`, created by `hydai`, stating `This issue pertains to the Typed function references proposal for WebAssembly, aiming to enhance function references for efficient indirect calls and better interoperability. The key goals include enabling direct function calls without runtime checks, representing function pointers without using tables, and facilitating the exchange of function references between modules and the host environment. Additionally, the proposal seeks to support safe closures and separate useful features from the GC proposal.\n\nTo address the requirements, the following tasks need to be completed:\n- Gain familiarity with the Wasm Spec\n- Study the Typed function references Spec\n- Integrate new type definitions and instructions in WasmEdge\n- Implement an option in WasmEdge CLI to enable/disable the proposal\n- Develop unit tests for comprehensive coverage\n\nTo qualify for the LFX mentorship, the applicant should have experience in C++ programming and complete challenge #1221.\n\nReferences:\n- GC Proposal: [WebAssembly GC](https://github.com/WebAssembly/gc)\n- Typed function references Proposal: [Function References](https://github.com/WebAssembly/function-references)`?\n\n### Response:";

    match completion_inner_async(query).await {
        Ok(res) => log::info!("res: {:?}", res),

        Err(_e) => log::error!("error: {:?}", _e),
    }

    Ok(())
}

pub async fn completion_inner_async(user_input: &str) -> anyhow::Result<String> {
    let llm_endpoint = "https://api-inference.huggingface.co/models/jaykchen/tiny";
    let llm_api_key = env::var("LLM_API_KEY").expect("LLM_API_KEY-must-be-set");
    let base_url = Uri::try_from(llm_endpoint).expect("Failed to parse URL");

    let mut writer = Vec::new(); // This will store the response body

    let query =
        json!({
        "inputs": user_input,
        "wait_for_model": true,
        "max_new_tokens": 500,
    });
    let query_bytes = serde_json::to_vec(&query).expect("Failed to serialize query to bytes");

    // let query_str = query.to_string();
    let query_len = query_bytes.len().to_string();
    // Prepare and send the HTTP request
    match
        Request::new(&base_url)
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
                return Err(anyhow::anyhow!("HTTP error with status {:?}", res.status_code()));
            }

            // Attempt to parse the response body into the expected structure
            let completion_response: Vec<Choice> = serde_json
                ::from_slice(&writer)
                .expect("Failed to parse response from API");

            if let Some(choice) = completion_response.get(0) {
                log::info!("Choice: {:?}", choice);
                Ok(choice.generated_text.clone())
            } else {
                Err(anyhow::anyhow!("No completion choices found in the response"))
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
