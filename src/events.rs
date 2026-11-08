use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct RawFile {
    messages: Vec<RawEvent>,
}

#[derive(Debug, Deserialize)]
struct RawEvent {
    id: String,
    #[serde(default)]
    text: String,
    #[serde(default)]
    file: Vec<Option<String>>,
    #[serde(default)]
    buttons: Vec<RawButton>,
    #[serde(default)]
    delay_minutes: i64,
    #[serde(default)]
    delay_seconds: i64,
}

#[derive(Debug, Deserialize)]
struct RawButton {
    #[serde(default)]
    id: serde_yaml::Value,
    text: String,
    #[serde(default)]
    action: String,
    #[serde(default)]
    event: String,
    /// Если задано — кнопка открывает ссылку (URL-кнопка, без callback).
    #[serde(default)]
    url: String,
}

#[derive(Debug, Clone)]
pub struct Event {
    pub id: String,
    pub text: String,
    pub files: Vec<String>,
    pub buttons: Vec<Button>,
    pub delay_minutes: i64,
    pub delay_seconds: i64,
}

#[derive(Debug, Clone)]
pub struct Button {
    pub id: String,
    pub text: String,
    pub action: String,
    pub event: String,
    pub url: String,
}

#[derive(Debug)]
pub struct Course {
    pub events: Vec<Event>,
    by_id: HashMap<String, usize>,
    data_dir: PathBuf,
}

impl Course {
    pub fn load(yaml_path: &Path, data_dir: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(yaml_path)
            .with_context(|| format!("read {}", yaml_path.display()))?;
        let raw: RawFile = serde_yaml::from_str(&content)
            .with_context(|| format!("parse {}", yaml_path.display()))?;

        let mut events = Vec::with_capacity(raw.messages.len());
        let mut by_id = HashMap::new();
        for (idx, e) in raw.messages.into_iter().enumerate() {
            let id = e.id.trim().to_string();
            let files: Vec<String> = e
                .file
                .into_iter()
                .flatten()
                .map(|f| f.trim().to_string())
                .filter(|f| !f.is_empty())
                .collect();
            let buttons: Vec<Button> = e
                .buttons
                .into_iter()
                .enumerate()
                .map(|(i, b)| Button {
                    id: button_id_to_string(&b.id, i),
                    text: b.text,
                    action: b.action,
                    event: b.event.trim().to_string(),
                    url: b.url.trim().to_string(),
                })
                .collect();

            if by_id.insert(id.clone(), idx).is_some() {
                tracing::warn!("duplicate event id in yaml: {}", id);
            }
            events.push(Event {
                id,
                text: e.text,
                files,
                buttons,
                delay_minutes: e.delay_minutes,
                delay_seconds: e.delay_seconds,
            });
        }
        Ok(Self {
            events,
            by_id,
            data_dir: data_dir.to_path_buf(),
        })
    }

    pub fn first(&self) -> Option<&Event> {
        self.events.first()
    }

    /// Итоговая задержка события (минуты + секунды) как chrono::Duration.
    pub fn delay_of(event: &Event) -> chrono::Duration {
        chrono::Duration::seconds(event.delay_minutes.max(0) * 60 + event.delay_seconds.max(0))
    }

    pub fn at(&self, idx: usize) -> Option<&Event> {
        self.events.get(idx)
    }

    pub fn index_of(&self, id: &str) -> Option<usize> {
        self.by_id.get(id.trim()).copied()
    }

    pub fn next_after_button(&self, current_idx: usize, button_event: &str) -> Option<usize> {
        let target = button_event.trim();
        if !target.is_empty() {
            if let Some(&idx) = self.by_id.get(target) {
                return Some(idx);
            }
            tracing::warn!(
                "button.event '{}' not found in yaml, falling back to next in order",
                target
            );
        }
        self.next_in_order(current_idx)
    }

    pub fn next_in_order(&self, current_idx: usize) -> Option<usize> {
        let next = current_idx + 1;
        if next < self.events.len() {
            Some(next)
        } else {
            None
        }
    }

    pub fn resolve_file(&self, name: &str) -> Option<PathBuf> {
        let p = self.data_dir.join(name);
        if p.exists() {
            Some(p)
        } else {
            tracing::warn!("attached file not found: {}", p.display());
            None
        }
    }
}

fn button_id_to_string(v: &serde_yaml::Value, fallback: usize) -> String {
    match v {
        serde_yaml::Value::String(s) => s.clone(),
        serde_yaml::Value::Number(n) => n.to_string(),
        serde_yaml::Value::Bool(b) => b.to_string(),
        _ => fallback.to_string(),
    }
}
