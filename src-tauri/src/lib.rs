use chrono::{DateTime, Duration, Utc};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use regex::Regex;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashSet;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration as StdDuration;
use walkdir::WalkDir;

const BUCKET_MINUTES: i64 = 10;

#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct TokenValues {
    input: u64,
    output: u64,
    cache_read: u64,
    cache_write: u64,
    reasoning: u64,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct UsageBucket {
    start: String,
    end: String,
    #[serde(flatten)]
    tokens: TokenValues,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RateLimit {
    used_percent: f64,
    resets_at: Option<i64>,
    source: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ToolUsage {
    five_hour: Option<RateLimit>,
    weekly: Option<RateLimit>,
    buckets: Vec<UsageBucket>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DashboardData {
    codex: ToolUsage,
    claude: ToolUsage,
    generated_at: String,
    warnings: Vec<String>,
}

fn json_u64(value: &Value, key: &str) -> u64 {
    value.get(key).and_then(Value::as_u64).unwrap_or(0)
}

fn normalized_tokens(usage: &Value, claude: bool) -> TokenValues {
    if claude {
        let reasoning = usage
            .pointer("/output_tokens_details/thinking_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let output_total = json_u64(usage, "output_tokens");
        TokenValues {
            input: json_u64(usage, "input_tokens"),
            output: output_total.saturating_sub(reasoning),
            cache_read: json_u64(usage, "cache_read_input_tokens"),
            cache_write: json_u64(usage, "cache_creation_input_tokens"),
            reasoning,
        }
    } else {
        let cached = json_u64(usage, "cached_input_tokens");
        let cache_write = json_u64(usage, "cache_write_input_tokens");
        let input_total = json_u64(usage, "input_tokens");
        let reasoning = json_u64(usage, "reasoning_output_tokens");
        let output_total = json_u64(usage, "output_tokens");
        TokenValues {
            input: input_total
                .saturating_sub(cached)
                .saturating_sub(cache_write),
            output: output_total.saturating_sub(reasoning),
            cache_read: cached,
            cache_write,
            reasoning,
        }
    }
}

fn add_tokens(target: &mut TokenValues, value: &TokenValues) {
    target.input += value.input;
    target.output += value.output;
    target.cache_read += value.cache_read;
    target.cache_write += value.cache_write;
    target.reasoning += value.reasoning;
}

fn empty_buckets(now: DateTime<Utc>, count: usize) -> Vec<UsageBucket> {
    let start = now - Duration::minutes(BUCKET_MINUTES * count as i64);
    (0..count)
        .map(|index| {
            let bucket_start = start + Duration::minutes(BUCKET_MINUTES * index as i64);
            UsageBucket {
                start: bucket_start.to_rfc3339(),
                end: (bucket_start + Duration::minutes(BUCKET_MINUTES)).to_rfc3339(),
                tokens: TokenValues::default(),
            }
        })
        .collect()
}

fn add_to_bucket(buckets: &mut [UsageBucket], timestamp: &str, tokens: &TokenValues) {
    let Ok(time) = DateTime::parse_from_rfc3339(timestamp) else {
        return;
    };
    let Some(first) = buckets
        .first()
        .and_then(|bucket| DateTime::parse_from_rfc3339(&bucket.start).ok())
    else {
        return;
    };
    let elapsed = time.signed_duration_since(first).num_seconds();
    if elapsed < 0 {
        return;
    }
    let index = elapsed / (BUCKET_MINUTES * 60);
    if let Some(bucket) = buckets.get_mut(index as usize) {
        add_tokens(&mut bucket.tokens, tokens);
    }
}

fn jsonl_files(root: &Path) -> Vec<PathBuf> {
    if !root.exists() {
        return Vec::new();
    }
    WalkDir::new(root)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| {
            entry.file_type().is_file()
                && entry.path().extension().is_some_and(|ext| ext == "jsonl")
        })
        .map(|entry| entry.into_path())
        .collect()
}

fn parse_codex(root: &Path, now: DateTime<Utc>, count: usize) -> ToolUsage {
    let mut buckets = empty_buckets(now, count);
    let mut latest_limits: Option<(DateTime<Utc>, Value)> = None;
    let mut seen_events = HashSet::new();
    for path in jsonl_files(root) {
        let Ok(file) = File::open(path) else { continue };
        for line in BufReader::new(file).lines().map_while(Result::ok) {
            let Ok(value) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            if value.pointer("/payload/type").and_then(Value::as_str) != Some("token_count") {
                continue;
            }
            let Some(timestamp) = value.get("timestamp").and_then(Value::as_str) else {
                continue;
            };
            if let Some(usage) = value.pointer("/payload/info/last_token_usage") {
                let total = value
                    .pointer("/payload/info/total_token_usage")
                    .unwrap_or(usage);
                let event_key = format!("{timestamp}:{total}");
                if seen_events.insert(event_key) {
                    add_to_bucket(&mut buckets, timestamp, &normalized_tokens(usage, false));
                }
            }
            if let (Ok(parsed), Some(limits)) = (
                timestamp.parse::<DateTime<Utc>>(),
                value.pointer("/payload/rate_limits"),
            ) {
                if latest_limits
                    .as_ref()
                    .is_none_or(|(current, _)| parsed > *current)
                {
                    latest_limits = Some((parsed, limits.clone()));
                }
            }
        }
    }
    let make_limit = |key: &str| {
        latest_limits
            .as_ref()
            .and_then(|(_, limits)| limits.get(key))
            .and_then(|limit| {
                Some(RateLimit {
                    used_percent: limit.get("used_percent")?.as_f64()?,
                    resets_at: limit.get("resets_at").and_then(Value::as_i64),
                    source: "Codex JSONL".into(),
                })
            })
    };
    ToolUsage {
        five_hour: make_limit("primary"),
        weekly: make_limit("secondary"),
        buckets,
    }
}

fn parse_claude(root: &Path, now: DateTime<Utc>, count: usize) -> Vec<UsageBucket> {
    let mut buckets = empty_buckets(now, count);
    let mut seen = HashSet::new();
    for path in jsonl_files(root) {
        let Ok(file) = File::open(path) else { continue };
        for line in BufReader::new(file).lines().map_while(Result::ok) {
            let Ok(value) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            let Some(usage) = value.pointer("/message/usage") else {
                continue;
            };
            let Some(timestamp) = value.get("timestamp").and_then(Value::as_str) else {
                continue;
            };
            let identity = value
                .pointer("/message/id")
                .and_then(Value::as_str)
                .or_else(|| value.get("requestId").and_then(Value::as_str));
            let Some(identity) = identity else { continue };
            if !seen.insert(identity.to_owned()) {
                continue;
            }
            add_to_bucket(&mut buckets, timestamp, &normalized_tokens(usage, true));
        }
    }
    buckets
}

fn parse_claude_usage(text: &str) -> (Option<RateLimit>, Option<RateLimit>) {
    let clean =
        String::from_utf8_lossy(&strip_ansi_escapes::strip(text.as_bytes())).replace('\r', "\n");
    let percent = Regex::new(r"(?i)(\d{1,3}(?:\.\d+)?)\s*%\s*(?:used)?").expect("valid regex");
    let mut five_hour = None;
    let mut weekly = None;
    let mut section = "";
    let mut lines_after_heading = 0_u8;
    for line in clean.lines() {
        let lower = line.to_lowercase();
        if lower.contains("session") || lower.contains("5 hour") || lower.contains("5-hour") {
            section = "five";
            lines_after_heading = 0;
            continue;
        }
        if lower.contains("week") || lower.contains("7 day") || lower.contains("7-day") {
            section = "week";
            lines_after_heading = 0;
            continue;
        }
        if section.is_empty() || line.trim().is_empty() {
            continue;
        }
        lines_after_heading += 1;
        if lines_after_heading > 4 {
            section = "";
            continue;
        }
        if let Some(found) = percent
            .captures(line)
            .and_then(|capture| capture.get(1))
            .and_then(|value| value.as_str().parse::<f64>().ok())
        {
            let limit = RateLimit {
                used_percent: found,
                resets_at: None,
                source: "Claude /usage".into(),
            };
            if section == "five" && five_hour.is_none() {
                five_hour = Some(limit);
            } else if section == "week" && weekly.is_none() {
                weekly = Some(limit);
            }
        }
    }
    (five_hour, weekly)
}

fn read_claude_usage() -> Result<(Option<RateLimit>, Option<RateLimit>), String> {
    let pty = native_pty_system();
    let pair = pty
        .openpty(PtySize {
            rows: 40,
            cols: 140,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|error| error.to_string())?;
    let mut command = CommandBuilder::new("claude");
    command.arg("--model");
    command.arg("sonnet");
    let mut child = pair
        .slave
        .spawn_command(command)
        .map_err(|error| error.to_string())?;
    drop(pair.slave);
    let mut reader = pair
        .master
        .try_clone_reader()
        .map_err(|error| error.to_string())?;
    let mut writer = pair
        .master
        .take_writer()
        .map_err(|error| error.to_string())?;
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = reader.read_to_end(&mut bytes);
        let _ = sender.send(bytes);
    });
    std::thread::sleep(StdDuration::from_millis(1800));
    writer
        .write_all(b"/usage\r")
        .map_err(|error| error.to_string())?;
    writer.flush().map_err(|error| error.to_string())?;
    std::thread::sleep(StdDuration::from_secs(4));
    writer.write_all(&[3]).map_err(|error| error.to_string())?;
    let _ = child.kill();
    drop(writer);
    drop(pair.master);
    let bytes = receiver
        .recv_timeout(StdDuration::from_secs(2))
        .unwrap_or_default();
    Ok(parse_claude_usage(&String::from_utf8_lossy(&bytes)))
}

fn collect_dashboard_data(bucket_count: usize) -> Result<DashboardData, String> {
    let count = bucket_count.clamp(1, 90);
    let home = dirs::home_dir().ok_or("ホームディレクトリを取得できませんでした")?;
    let now = Utc::now();
    let codex = parse_codex(&home.join(".codex").join("sessions"), now, count);
    let claude_buckets = parse_claude(&home.join(".claude").join("projects"), now, count);
    let mut warnings = Vec::new();
    let (usage_sender, usage_receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = usage_sender.send(read_claude_usage());
    });
    let (five_hour, weekly) = match usage_receiver.recv_timeout(StdDuration::from_secs(10)) {
        Ok(Ok(limits)) => limits,
        Err(_) => {
            warnings.push("Claude /usageの取得が10秒以内に完了しませんでした。".into());
            (None, None)
        }
        Ok(Err(error)) => {
            warnings.push(format!("Claude /usageを取得できませんでした: {error}"));
            (None, None)
        }
    };
    if five_hour.is_none() || weekly.is_none() {
        warnings.push("Claude /usageの上限使用率を解析できませんでした。Claude CLIの表示形式を確認してください。".into());
    }
    Ok(DashboardData {
        codex,
        claude: ToolUsage {
            five_hour,
            weekly,
            buckets: claude_buckets,
        },
        generated_at: now.to_rfc3339(),
        warnings,
    })
}

#[tauri::command]
async fn get_dashboard_data(bucket_count: usize) -> Result<DashboardData, String> {
    tauri::async_runtime::spawn_blocking(move || collect_dashboard_data(bucket_count))
        .await
        .map_err(|error| format!("使用量データの収集処理に失敗しました: {error}"))?
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let mut builder = tauri::Builder::default();
    #[cfg(debug_assertions)]
    {
        builder = builder.plugin(
            tauri_plugin_mcp_bridge::Builder::new()
                .bind_address("127.0.0.1")
                .build(),
        );
    }
    builder
        .invoke_handler(tauri::generate_handler![get_dashboard_data])
        .run(tauri::generate_context!())
        .expect("error while running Tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_codex_subsets_without_double_counting() {
        let usage = serde_json::json!({"input_tokens":100,"cached_input_tokens":30,"cache_write_input_tokens":10,"output_tokens":25,"reasoning_output_tokens":5});
        let result = normalized_tokens(&usage, false);
        assert_eq!(result.input, 60);
        assert_eq!(result.output, 20);
        assert_eq!(result.cache_read, 30);
        assert_eq!(result.cache_write, 10);
        assert_eq!(result.reasoning, 5);
    }

    #[test]
    fn parses_claude_percentages() {
        let text = "Current session\n42% used\nCurrent week (all models)\n67% used";
        let (five, week) = parse_claude_usage(text);
        assert_eq!(five.unwrap().used_percent, 42.0);
        assert_eq!(week.unwrap().used_percent, 67.0);
    }
}
