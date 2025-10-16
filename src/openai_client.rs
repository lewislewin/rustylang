use anyhow::{anyhow, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Semaphore;

pub struct OpenAiTranslator {
    client: Client,
    api_key: String,
    model: String,
    gate: Arc<Semaphore>,
}

impl OpenAiTranslator {
    pub fn new(api_key: String, model: String, concurrency: usize) -> Result<Self> {
        if api_key.is_empty() { return Err(anyhow!("OPENAI_API_KEY is empty")); }
        let client = Client::builder().build()?;
        Ok(Self { client, api_key, model, gate: Arc::new(Semaphore::new(concurrency.max(1))) })
    }

    pub async fn translate(&self, text: &str, source_locale: &str, target_locale: &str, required_placeholders: &[String]) -> Result<String> {
        let _permit = self.gate.acquire().await;
        let system = format!(
            "You are a professional localization engine. Translate from {} to {}. Keep placeholders such as {{}}, {{like_this}}, :named, %s unchanged. Return text only, no quotes.",
            source_locale, target_locale
        );
        let placeholders_note = if required_placeholders.is_empty() {
            String::new()
        } else {
            format!("\nRequired placeholders (must appear unchanged): {}", required_placeholders.join(", "))
        };
        let user = format!("{}{}", text, placeholders_note);

        let body = ChatRequest {
            model: self.model.clone(),
            messages: vec![
                ChatMessage { role: "system".into(), content: system },
                ChatMessage { role: "user".into(), content: user },
            ],
            temperature: 1.0,
        };

        // Simple retry with backoff (3 attempts)
        let mut last_err: Option<anyhow::Error> = None;
        for attempt in 0..3 {
            let res = self.client
                .post("https://api.openai.com/v1/chat/completions")
                .bearer_auth(&self.api_key)
                .json(&body)
                .send()
                .await;

            match res {
                Ok(resp) => {
                    if resp.status().is_success() {
                        let data: ChatResponse = resp.json().await?;
                        let answer = data.choices.get(0).and_then(|c| c.message.content.clone()).unwrap_or_default();
                        return Ok(answer.trim().to_string());
                    } else {
                        let status = resp.status();
                        let txt = resp.text().await.unwrap_or_default();
                        last_err = Some(anyhow!("OpenAI error {}: {}", status, txt));
                    }
                }
                Err(e) => { last_err = Some(e.into()); }
            }
            // backoff
            let delay_ms = 200 * (attempt + 1) as u64;
            tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
        }
        Err(last_err.unwrap_or_else(|| anyhow!("Unknown OpenAI error")))
    }
}

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    temperature: f32,
}

#[derive(Serialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: ChoiceMessage,
}

#[derive(Deserialize)]
struct ChoiceMessage {
    content: Option<String>,
}


