use anyhow::{anyhow, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;
use tokio::sync::Semaphore;

#[derive(Clone)]
pub struct OpenAiTranslator {
    client: Client,
    api_key: String,
    model: String,
    gate: Arc<Semaphore>,
    usage: Arc<UsageCounters>,
    usage_by_locale: Arc<Mutex<HashMap<String, Arc<UsageCounters>>>>,
}

impl OpenAiTranslator {
    pub fn new(api_key: String, model: String, concurrency: usize) -> Result<Self> {
        if api_key.is_empty() { return Err(anyhow!("OPENAI_API_KEY is empty")); }
        let client = Client::builder()
            .user_agent("rustylang/0.1.0 (+https://github.com/)")
            .timeout(Duration::from_secs(30))
            .build()?;
        Ok(Self {
            client,
            api_key,
            model,
            gate: Arc::new(Semaphore::new(concurrency.max(1))),
            usage: Arc::new(UsageCounters::default()),
            usage_by_locale: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    pub async fn translate(
        &self,
        key_path: Option<&str>,
        text: &str,
        source_locale: &str,
        target_locale: &str,
        required_placeholders: &[String],
    ) -> Result<String> {
        // Hold the permit for the duration of the request; drops at end of scope
        let _permit = self.gate.acquire().await;
        // Build strict system instructions so the model returns ONLY the translation
        let mut system = format!(
            concat!(
                "You are a professional localization engine.\n",
                "- Translate from {} to {}.\n",
                "- Preserve placeholders unchanged (verbatim), e.g. {{like_this}}, :named, %s, {{...}}, {{...}}.\n",
                "- Output MUST be only the translated text: no quotes, no code fences, no labels, no explanations. unless the text is a placeholder.\n",
                "- Do NOT echo instructions or placeholder lists.\n",
            ),
            source_locale,
            target_locale,
        );
        if !required_placeholders.is_empty() {
            let list = required_placeholders.join(", ");
            system.push_str(&format!(
                "- Required placeholders (must appear verbatim): {}\n",
                list
            ));
        }
        if let Some(k) = key_path {
            system.push_str(&format!(
                "- Key (context only; do not output. Only use for context and if you are unsure about the translation): {}\n",
                k
            ));
        }
        // User message is ONLY the source text to translate
        let user = text.to_string();

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
                        if let Some(u) = data.usage.as_ref() {
                            // Global counters
                            if let Some(v) = u.prompt_tokens { self.usage.prompt_tokens.fetch_add(v as u64, Ordering::Relaxed); }
                            if let Some(v) = u.completion_tokens { self.usage.completion_tokens.fetch_add(v as u64, Ordering::Relaxed); }
                            if let Some(v) = u.total_tokens { self.usage.total_tokens.fetch_add(v as u64, Ordering::Relaxed); }
                            self.usage.requests.fetch_add(1, Ordering::Relaxed);

                            // Per-locale counters
                            let per_arc = {
                                let mut map = self.usage_by_locale.lock().unwrap();
                                map.entry(target_locale.to_string())
                                    .or_insert_with(|| Arc::new(UsageCounters::default()))
                                    .clone()
                            };
                            if let Some(v) = u.prompt_tokens { per_arc.prompt_tokens.fetch_add(v as u64, Ordering::Relaxed); }
                            if let Some(v) = u.completion_tokens { per_arc.completion_tokens.fetch_add(v as u64, Ordering::Relaxed); }
                            if let Some(v) = u.total_tokens { per_arc.total_tokens.fetch_add(v as u64, Ordering::Relaxed); }
                            per_arc.requests.fetch_add(1, Ordering::Relaxed);
                        } else {
                            // Count request even if usage absent
                            self.usage.requests.fetch_add(1, Ordering::Relaxed);
                            let per_arc = {
                                let mut map = self.usage_by_locale.lock().unwrap();
                                map.entry(target_locale.to_string())
                                    .or_insert_with(|| Arc::new(UsageCounters::default()))
                                    .clone()
                            };
                            per_arc.requests.fetch_add(1, Ordering::Relaxed);
                        }
                        let raw = data
                            .choices
                            .get(0)
                            .and_then(|c| c.message.content.clone())
                            .unwrap_or_default();
                        // Sanitize: take first non-empty line and strip surrounding quotes
                        let mut first_line = raw
                            .lines()
                            .find(|l| !l.trim().is_empty())
                            .unwrap_or("")
                            .trim()
                            .to_string();
                        // Strip common surrounding quotes if present
                        for (lq, rq) in [("\"", "\""), ("“", "”"), ("'", "'")] {
                            if first_line.starts_with(lq) && first_line.ends_with(rq) && first_line.len() >= lq.len() + rq.len() {
                                first_line = first_line[lq.len()..first_line.len() - rq.len()].trim().to_string();
                            }
                        }
                        return Ok(first_line);
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
    usage: Option<Usage>,
}

#[derive(Deserialize)]
struct Choice {
    message: ChoiceMessage,
}

#[derive(Deserialize)]
struct ChoiceMessage {
    content: Option<String>,
}

#[derive(Default)]
struct UsageCounters {
    prompt_tokens: AtomicU64,
    completion_tokens: AtomicU64,
    total_tokens: AtomicU64,
    requests: AtomicU64,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct TokenUsageSnapshot {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    pub requests: u64,
}

impl OpenAiTranslator {
    pub fn usage_snapshot(&self) -> TokenUsageSnapshot {
        TokenUsageSnapshot {
            prompt_tokens: self.usage.prompt_tokens.load(Ordering::Relaxed),
            completion_tokens: self.usage.completion_tokens.load(Ordering::Relaxed),
            total_tokens: self.usage.total_tokens.load(Ordering::Relaxed),
            requests: self.usage.requests.load(Ordering::Relaxed),
        }
    }

    pub fn usage_by_locale_snapshot(&self) -> Vec<(String, TokenUsageSnapshot)> {
        let map = self.usage_by_locale.lock().unwrap();
        map.iter()
            .map(|(k, v)| {
                (
                    k.clone(),
                    TokenUsageSnapshot {
                        prompt_tokens: v.prompt_tokens.load(Ordering::Relaxed),
                        completion_tokens: v.completion_tokens.load(Ordering::Relaxed),
                        total_tokens: v.total_tokens.load(Ordering::Relaxed),
                        requests: v.requests.load(Ordering::Relaxed),
                    },
                )
            })
            .collect()
    }
}

#[derive(Deserialize)]
struct Usage {
    prompt_tokens: Option<u64>,
    completion_tokens: Option<u64>,
    total_tokens: Option<u64>,
}


