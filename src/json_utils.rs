use crate::errors::RustyLangError;
use anyhow::{Context, Result};
use serde_json::{Map, Value};
use std::fs;
use std::path::Path;

pub fn read_json_file(path: &Path) -> Result<Value> {
    if !path.exists() {
        return Ok(Value::Object(Map::new()));
    }
    let s = fs::read_to_string(path).with_context(|| format!("Reading {:?}", path))?;
    let v: Value = serde_json::from_str(&s).with_context(|| format!("Parsing JSON {:?}", path))?;
    Ok(v)
}

pub fn write_json_atomic(path: &Path, json: &Value) -> Result<()> {
    let pretty = serde_json::to_string_pretty(json)?;
    let tmp_path = path.with_extension("tmp");
    // backup
    let bak_path = path.with_extension("bak");
    if path.exists() && !bak_path.exists() {
        fs::copy(path, &bak_path).ok();
    }
    fs::write(&tmp_path, pretty)?;
    fs::rename(&tmp_path, path)?;
    Ok(())
}

#[derive(Debug, Clone)]
enum PathSegment {
    Key(String),
    Index(usize),
}

fn parse_dot_path(path: &str) -> Result<Vec<PathSegment>> {
    // supports escaping dot as \. and array indices like [0]
    let mut segments: Vec<PathSegment> = Vec::new();
    let mut buf = String::new();
    let mut chars = path.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                if let Some(next) = chars.next() { buf.push(next); } else { buf.push('\\'); }
            }
            '.' => {
                if !buf.is_empty() { segments.push(PathSegment::Key(buf.clone())); buf.clear(); }
            }
            '[' => {
                // flush key buffer if present
                if !buf.is_empty() { segments.push(PathSegment::Key(buf.clone())); buf.clear(); }
                // parse number until ']'
                let mut num = String::new();
                while let Some(nc) = chars.next() {
                    if nc == ']' { break; }
                    num.push(nc);
                }
                let idx: usize = num.parse().map_err(|_| RustyLangError::InvalidDotPath(path.to_string()))?;
                segments.push(PathSegment::Index(idx));
            }
            _ => buf.push(c),
        }
    }
    if !buf.is_empty() { segments.push(PathSegment::Key(buf)); }
    Ok(segments)
}

pub fn set_value_at_path(root: &mut Value, path: &str, value: Value, create_missing: bool) -> Result<()> {
    let segments = parse_dot_path(path)?;
    let mut current = root;
    for (i, seg) in segments.iter().enumerate() {
        let is_last = i == segments.len() - 1;
        match seg {
            PathSegment::Key(k) => {
                if is_last {
                    ensure_object(current)?;
                    if let Value::Object(map) = current { map.insert(k.clone(), value); }
                    return Ok(());
                }
                match current {
                    Value::Object(map) => {
                        if !map.contains_key(k) {
                            if create_missing { map.insert(k.clone(), Value::Object(Map::new())); }
                            else { return Err(RustyLangError::PathNotFound(path.to_string()).into()); }
                        }
                        current = map.get_mut(k).unwrap();
                    }
                    _ => return Err(RustyLangError::InvalidDotPath(path.to_string()).into()),
                }
            }
            PathSegment::Index(idx) => {
                // arrays: convert current to array if create_missing
                if is_last {
                    ensure_array(current, *idx, create_missing)?;
                    if let Value::Array(arr) = current { if *idx >= arr.len() { arr.resize(*idx + 1, Value::Null); } arr[*idx] = value; }
                    return Ok(());
                }
                match current {
                    Value::Array(arr) => {
                        if *idx >= arr.len() {
                            if create_missing { arr.resize(*idx + 1, Value::Object(Map::new())); }
                            else { return Err(RustyLangError::PathNotFound(path.to_string()).into()); }
                        }
                        current = &mut arr[*idx];
                    }
                    Value::Null if create_missing => {
                        *current = Value::Array(vec![]);
                        ensure_array(current, *idx, true)?;
                        if let Value::Array(arr) = current { current = &mut arr[*idx]; }
                    }
                    _ => return Err(RustyLangError::InvalidDotPath(path.to_string()).into()),
                }
            }
        }
    }
    Ok(())
}

fn ensure_object(v: &mut Value) -> Result<()> {
    if matches!(v, Value::Object(_)) { return Ok(()); }
    if v.is_null() { *v = Value::Object(Map::new()); return Ok(()); }
    Err(RustyLangError::InvalidDotPath("expected object".into()).into())
}

fn ensure_array(v: &mut Value, min_index: usize, create_missing: bool) -> Result<()> {
    match v {
        Value::Array(arr) => {
            if arr.len() <= min_index { arr.resize(min_index + 1, Value::Null); }
            Ok(())
        }
        Value::Null if create_missing => {
            *v = Value::Array(vec![Value::Null; min_index + 1]);
            Ok(())
        }
        _ => Err(RustyLangError::InvalidDotPath("expected array".into()).into()),
    }
}


