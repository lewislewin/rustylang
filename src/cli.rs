use crate::config::load_config;
use crate::diff::{compute_missing_translations, flatten_string_paths};
use crate::json_utils::{read_json_file, set_value_at_path, write_json_atomic};
use crate::openai_client::OpenAiTranslator;
use anyhow::{anyhow, Context, Result};
use clap::{Args, Parser, Subcommand};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use futures::{stream, StreamExt};
use serde_json::Value;
use std::env;
use std::sync::Arc;
use std::path::PathBuf;
use tracing::{error, info};
use regex::Regex;

#[derive(Parser, Debug)]
#[command(name = "rustylang", version, about = "i18n helper CLI")] 
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Set/update a string in the source locale file (default en-GB.json)
    Set(SetArgs),
    /// Translate missing (or all with --overwrite) strings for configured locales
    Translate(TranslateArgs),
}

#[derive(Args, Debug)]
pub struct SetArgs {
    /// Dot path (supports escaping with \\.) e.g. flows.general-title
    pub path: String,
    /// Text to set (string)
    pub text: String,
    /// File to edit (defaults to source locale from config, usually en-GB.json)
    #[arg(long)]
    pub file: Option<PathBuf>,
    /// Disable creating intermediate objects/arrays automatically
    #[arg(long)]
    pub no_create_missing: bool,
}

#[derive(Args, Debug)]
pub struct TranslateArgs {
    /// Comma-separated locales to translate (overrides config)
    #[arg(long)]
    pub locales: Option<String>,
    /// Concurrency for API calls
    #[arg(long)]
    pub concurrency: Option<usize>,
    /// Overwrite existing translations
    #[arg(long)]
    pub overwrite: bool,
    /// Dry run: show planned changes only
    #[arg(long)]
    pub dry_run: bool,
    /// Model override (defaults from config)
    #[arg(long)]
    pub model: Option<String>,
}

pub async fn handle_set(args: SetArgs) -> Result<()> {
    let cfg = load_config()?;
    let file = args.file.unwrap_or_else(|| {
        PathBuf::from(cfg.file_pattern.replace("{locale}", &cfg.source_locale))
    });

    // Read file
    let mut json = read_json_file(&file).with_context(|| format!("Reading {:?}", file))?;

    // Update
    // Create intermediate objects by default for better UX
    let create_missing = !args.no_create_missing;
    set_value_at_path(&mut json, &args.path, Value::String(args.text.clone()), create_missing)
        .with_context(|| format!("Setting {} in {:?}", args.path, file))?;

    // Write atomically
    write_json_atomic(&file, &json).with_context(|| format!("Writing {:?}", file))?;

    info!(path=?args.path, file=?file, "Updated translation");
    Ok(())
}

pub async fn handle_translate(args: TranslateArgs) -> Result<()> {
    let mut cfg = load_config()?;
    if let Some(c) = args.concurrency { cfg.concurrency = c; }
    if let Some(m) = args.model.clone() { cfg.openai.model = m; }

    let locales: Vec<String> = match args.locales.as_ref() {
        Some(s) => s.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect(),
        None => cfg.locales.clone(),
    };
    if locales.is_empty() {
        return Err(anyhow!("No locales specified (config or --locales)"));
    }

    let source_file = PathBuf::from(cfg.file_pattern.replace("{locale}", &cfg.source_locale));
    let source = read_json_file(&source_file)
        .with_context(|| format!("Reading source file {:?}", source_file))?;
    let source_flat = flatten_string_paths(&source, None);
    if source_flat.is_empty() {
        return Err(anyhow!("No string leaves found in source {:?}", source_file));
    }

    // Translator setup
    let api_key = env::var("OPENAI_API_KEY")
        .ok()
        .or(cfg.openai.api_key.clone())
        .unwrap_or_default();
    if api_key.is_empty() {
        return Err(anyhow!("OPENAI_API_KEY not set and no key in config"));
    }
    let translator = OpenAiTranslator::new(api_key, cfg.openai.model.clone(), cfg.concurrency)?;

    let mp = MultiProgress::new();
    let pb_style = ProgressStyle::with_template("{msg} {bar:40.cyan/blue} {pos}/{len}")
        .unwrap()
        .progress_chars("##-");

    // Process locales concurrently (bounded by cfg.concurrency)
    let mp = Arc::new(mp);
    let translator = translator.clone();
    let file_pattern = cfg.file_pattern.clone();
    let source_locale = cfg.source_locale.clone();
    let concurrency = cfg.concurrency;
    let results = stream::iter(locales.into_iter())
        .map(|locale| {
            let translator = translator.clone();
            let mp = mp.clone();
            let pb_style = pb_style.clone();
            let source = source.clone();
            let file_pattern = file_pattern.clone();
            let source_locale = source_locale.clone();
            async move {
                if locale == source_locale { return Ok::<(), anyhow::Error>(()); }
                let target_file = PathBuf::from(file_pattern.replace("{locale}", &locale));
                let mut target = read_json_file(&target_file).unwrap_or(Value::Object(serde_json::Map::new()));
                let to_fill = compute_missing_translations(&source, &target, args.overwrite);
                if to_fill.is_empty() {
                    info!(locale=%locale, "No translations needed");
                    return Ok(());
                }

                let pb = mp.add(ProgressBar::new(to_fill.len() as u64));
                pb.set_style(pb_style.clone());
                pb.set_message(format!("{}", locale));

                let updates = stream::iter(to_fill.into_iter())
                    .map(|(path, english)| {
                        let translator = translator.clone();
                        let source_locale = source_locale.clone();
                        let locale = locale.clone();
                        async move {
                            if args.dry_run {
                                return Ok::<(String, String), anyhow::Error>((path, String::from("<translated>")));
                            }
                            let placeholders = extract_placeholders(&english);
                            match translator.translate(Some(&path), &english, &source_locale, &locale, &placeholders).await {
                                Ok(tx) => Ok((path, tx)),
                                Err(err) => {
                                    error!(?err, path=%path, "Translation failed, using source text");
                                    Ok((path, english))
                                }
                            }
                        }
                    })
                    .buffer_unordered(concurrency)
                    .inspect(|_| pb.inc(1))
                    .collect::<Vec<_>>()
                    .await;

                pb.finish_and_clear();
                if args.dry_run { info!(locale=%locale, count=%updates.len(), "Dry run: would update keys"); return Ok(()); }

                for item in updates.into_iter() {
                    let (path, txt) = item?;
                    set_value_at_path(&mut target, &path, Value::String(txt), true)?;
                }
                write_json_atomic(&target_file, &target)?;
                info!(locale=%locale, file=?target_file, "Wrote translations");
                Ok(())
            }
        })
        .buffer_unordered(concurrency)
        .collect::<Vec<_>>()
        .await;

    for res in results { res?; }

    // Token usage summary
    let usage = translator.usage_snapshot();
    info!(
        prompt_tokens=%usage.prompt_tokens,
        completion_tokens=%usage.completion_tokens,
        total_tokens=%usage.total_tokens,
        requests=%usage.requests,
        "OpenAI usage summary"
    );

    // Human-readable stdout summary
    println!(
        "\nUsage summary: total={} (prompt={}, completion={}), requests={}",
        usage.total_tokens, usage.prompt_tokens, usage.completion_tokens, usage.requests
    );

    // Per-locale breakdown
    let mut per = translator.usage_by_locale_snapshot();
    per.sort_by(|a, b| a.0.cmp(&b.0));
    if !per.is_empty() {
        println!("Per-locale usage:");
        for (loc, u) in per {
            println!(
                "  {}: total={}, prompt={}, completion={}, requests={}",
                loc, u.total_tokens, u.prompt_tokens, u.completion_tokens, u.requests
            );
        }
    }

    Ok(())
}

fn extract_placeholders(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    // Patterns: {word}, {{mustache}}, :named, %s, %d, {0}, {name}
    let patterns = vec![
        Regex::new(r"\{\{[^}]+\}\}").unwrap(),
        Regex::new(r"\{[^}]+\}").unwrap(),
        Regex::new(r":[A-Za-z_][A-Za-z0-9_]*").unwrap(),
        Regex::new(r"%[sd]?").unwrap(),
    ];
    for re in patterns.iter() {
        for m in re.find_iter(s) {
            let p = m.as_str().to_string();
            if !out.contains(&p) { out.push(p); }
        }
    }
    out
}


