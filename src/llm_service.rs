use anyhow;
use flowsnet_platform_sdk::logger;
use itertools::Itertools;
use llmservice_flows::{chat::ChatOptions, LLMServiceFlows};
use openai_flows::{embeddings::EmbeddingsInput, OpenAIFlows};
use regex::Regex;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use vector_store_flows::*;

pub async fn llm_chat(system_prompt: &str, user_prompt: &str) -> anyhow::Result<String> {
    let llm_endpoint = std::env::var("llm_endpoint").unwrap_or("".to_string());
    let llm_api_key = std::env::var("LLM_API_KEY").unwrap_or("".to_string());
    let mut llm = LLMServiceFlows::new(&llm_endpoint);
    llm.set_api_key(&llm_api_key);

    let co = ChatOptions {
        model: Some("Phind/Phind-CodeLlama-34B-v2"),
        // model: Some("mistralai/Mixtral-8x7B-Instruct-v0.1"),
        restart: false,
        system_prompt: Some(system_prompt),
        post_prompt: None,
        token_limit: 2048,
        ..Default::default()
    };

    match llm.chat_completion("chat_id", user_prompt, &co).await {
        Ok(r) => Ok(r.choice),
        Err(e) => {
            log::error!("LLM returns error: {}", e);
            Err(anyhow::anyhow!("LLM returns error: {}", e))
        }
    }
}

pub async fn search_collection(
    question: &str,
    collection_name: &str,
) -> anyhow::Result<Vec<(u64, String)>> {
    let mut openai = OpenAIFlows::new();
    openai.set_retry_times(3);

    let question_vector = match openai
        .create_embeddings(EmbeddingsInput::String(question.to_string()))
        .await
    {
        Ok(r) => {
            if r.len() < 1 {
                log::error!("LLM returned no embedding for the question");
                return Err(anyhow::anyhow!(
                    "LLM returned no embedding for the question"
                ));
            }
            r[0].iter().map(|n| *n as f32).collect()
        }
        Err(_e) => {
            log::error!("LLM returned an error: {}", _e);
            return Err(anyhow::anyhow!(
                "LLM returned no embedding for the question"
            ));
        }
    };

    let p = PointsSearchParams {
        vector: question_vector,
        limit: 5,
    };
    let mut rag_content = Vec::new();

    match search_points(&collection_name, &p).await {
        Ok(sp) => {
            for p in sp.iter() {
                let p_text = p
                    .payload
                    .as_ref()
                    .unwrap()
                    .get("text")
                    .unwrap()
                    .as_str()
                    .unwrap();
                let p_id = match p.id {
                    PointId::Num(i) => i,
                    _ => 0,
                };
                if p.score > 0.75 {
                    rag_content.push((p_id, p_text.to_string()));
                }
            }
        }
        Err(e) => {
            log::error!("Vector search returns error: {}", e);
        }
    }
    Ok(rag_content)
}

pub async fn get_rag_content(text: &str, hypo_answer: &str) -> anyhow::Result<String> {
    let raw_found_vec = search_collection(&text, "collect_name").await?;

    let mut raw_found_combined = raw_found_vec.into_iter().collect::<HashMap<u64, String>>();

    // use the additional source material found to update the context for answer generation
    let found_vec = search_collection(&hypo_answer, "collect_name").await?;

    for (id, text) in found_vec {
        raw_found_combined.insert(id, text);
    }

    let found_combined = raw_found_combined
        .into_iter()
        .map(|(_, v)| v)
        .collect::<Vec<String>>()
        .join("\n");

    Ok(found_combined)
}

pub async fn is_relevant(current_q: &str, previous_q: &str) -> bool {
    use nalgebra::DVector;

    let mut openai = OpenAIFlows::new();
    openai.set_retry_times(3);

    let embedding_input = EmbeddingsInput::Vec(vec![current_q.to_string(), previous_q.to_string()]);

    let (current_q_vector, previous_q_vector) =
        match openai.create_embeddings(embedding_input).await {
            Ok(r) if r.len() >= 2 => r
                .into_iter()
                .map(|v| v.iter().map(|&n| n as f32).collect::<Vec<f32>>())
                .take(2)
                .collect_tuple()
                .unwrap_or((Vec::<f32>::new(), Vec::<f32>::new())),
            _ => {
                log::error!("LLM returned an error");
                return false;
            }
        };

    let q1 = DVector::from_vec(current_q_vector);
    let q2 = DVector::from_vec(previous_q_vector);
    let score = q1.dot(&q2);

    let head = current_q.chars().take(100).collect::<String>();
    let tail = previous_q.chars().take(100).collect::<String>();
    log::debug!("similarity: {score} between {head} and {tail}");

    score > 0.75
}

pub async fn create_ephemeral_collection() {
    let collection_name = "ephemeral";
    let vector_size: u64 = 1536;

    let p = CollectionCreateParams {
        vector_size: vector_size,
    };
    if let Err(e) = create_collection(collection_name, &p).await {
        log::error!(
            "Cannot create collection named: {} with error: {}",
            collection_name,
            e
        );
        return;
    }
}

pub async fn reset_ephemeral_collection() {
    let collection_name = "ephemeral";
    let vector_size: u64 = 1536;

    _ = delete_collection(collection_name).await;

    let p = CollectionCreateParams {
        vector_size: vector_size,
    };
    if let Err(e) = create_collection(collection_name, &p).await {
        log::error!(
            "Cannot create collection named: {} with error: {}",
            collection_name,
            e
        );
        return;
    }
}

pub async fn upsert_text(text_to_upsert: &str) {
    let mut points = Vec::<Point>::new();
    let openai = OpenAIFlows::new();
    let collection_name = "ephemeral";
    let id = match collection_info(collection_name).await {
        Ok(ci) => ci.points_count + 1,
        Err(e) => {
            log::error!("Cannot get collection stat {}", e);
            return;
        }
    };

    let input = EmbeddingsInput::String(text_to_upsert.to_string());
    match openai.create_embeddings(input).await {
        Ok(r) => {
            let p = Point {
                id: PointId::Num(id),
                vector: r[0].iter().map(|n| *n as f32).collect(),
                payload: json!({"text": text_to_upsert})
                    .as_object()
                    .map(|m| m.to_owned()),
            };
            points.push(p);
        }
        Err(e) => {
            log::error!("OpenAI returned an error: {}", e);
        }
    }

    if let Err(e) = upsert_points(collection_name, points).await {
        log::error!("Cannot upsert into database! {}", e);
        return;
    }
}

pub fn parse_issue_summary_advanced(input: &str) -> anyhow::Result<HashMap<String, String>> {
    match serde_json::from_str::<Value>(input) {
        Ok(parsed) => process_json_object(parsed),
        Err(_) => {
            let first_brace_index = input.find('{');
            if let Some(start_index) = first_brace_index {
                let mut last_comma_index = None;
                let mut chars = input.char_indices().rev().peekable();
                while let Some((i, c)) = chars.next() {
                    if c == ',' && chars.peek().map_or(false, |&(_, next_c)| next_c == '"') {
                        last_comma_index = Some(i);
                        break;
                    }
                }

                if let Some(comma_index) = last_comma_index {
                    let corrected_input = format!("{}\"}}", &input[start_index..=comma_index]);
                    match serde_json::from_str::<Value>(&corrected_input) {
                        Ok(parsed) => process_json_object(parsed),
                        Err(e) => Err(anyhow::anyhow!("Failed to parse after correction: {}", e)),
                    }
                } else {
                    Err(anyhow::anyhow!("Unable to locate a valid JSON structure"))
                }
            } else {
                Err(anyhow::anyhow!(
                    "Malformed JSON: Missing opening curly brace"
                ))
            }
        }
    }
}

fn process_json_object(parsed: Value) -> anyhow::Result<HashMap<String, String>> {
    if let Value::Object(obj) = parsed {
        let summaries = obj
            .into_iter()
            .filter_map(|(key, value)| {
                if let Value::String(summary_str) = value {
                    Some((key, summary_str))
                } else {
                    None
                }
            })
            .collect::<HashMap<String, String>>();

        Ok(summaries)
    } else {
        Err(anyhow::anyhow!("Parsed JSON is not an object"))
    }
}
