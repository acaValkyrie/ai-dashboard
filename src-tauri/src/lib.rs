use chrono::{DateTime, Duration, Utc};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashSet;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::Command;
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
    claude_login_required: bool,
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

fn jsonl_files(root: &Path, cutoff: DateTime<Utc>, include_newest: bool) -> Vec<PathBuf> {
    if !root.exists() {
        return Vec::new();
    }
    let files: Vec<(PathBuf, Option<DateTime<Utc>>)> = WalkDir::new(root)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| {
            entry.file_type().is_file()
                && entry.path().extension().is_some_and(|ext| ext == "jsonl")
        })
        .map(|entry| {
            let modified = entry
                .metadata()
                .ok()
                .and_then(|metadata| metadata.modified().ok())
                .map(DateTime::<Utc>::from);
            (entry.into_path(), modified)
        })
        .collect();
    let newest = include_newest
        .then(|| {
            files
                .iter()
                .filter_map(|(path, modified)| modified.map(|time| (path, time)))
                .max_by_key(|(_, time)| *time)
                .map(|(path, _)| path.clone())
        })
        .flatten();
    files
        .into_iter()
        .filter(|(path, modified)| {
            modified.is_none_or(|time| time >= cutoff) || newest.as_ref() == Some(path)
        })
        .map(|(path, _)| path)
        .collect()
}

fn parse_codex(root: &Path, now: DateTime<Utc>, count: usize) -> ToolUsage {
    let mut buckets = empty_buckets(now, count);
    let cutoff = now - Duration::minutes(BUCKET_MINUTES * count as i64 + 60);
    let mut latest_limits: Option<(DateTime<Utc>, Value)> = None;
    let mut seen_events = HashSet::new();
    for path in jsonl_files(root, cutoff, true) {
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
    let cutoff = now - Duration::minutes(BUCKET_MINUTES * count as i64 + 60);
    let mut seen = HashSet::new();
    for path in jsonl_files(root, cutoff, false) {
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

fn parse_claude_limit(usage: &Value, key: &str) -> Option<RateLimit> {
    let limit = usage.get(key)?;
    let resets_at = limit
        .get("resets_at")
        .and_then(Value::as_str)
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.timestamp());
    Some(RateLimit {
        used_percent: limit.get("utilization")?.as_f64()?,
        resets_at,
        source: "Anthropic OAuth usage API".into(),
    })
}

fn read_claude_limits(
    home: &Path,
) -> Result<(Option<RateLimit>, Option<RateLimit>), (String, bool)> {
    let credentials_path = home.join(".claude").join(".credentials.json");
    let credentials: Value = serde_json::from_reader(
        File::open(&credentials_path)
            .map_err(|error| (format!("Claude認証情報を開けませんでした: {error}"), true))?,
    )
    .map_err(|error| {
        (
            format!("Claude認証情報を解析できませんでした: {error}"),
            true,
        )
    })?;
    let access_token = credentials
        .pointer("/claudeAiOauth/accessToken")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            (
                "Claude OAuthアクセストークンが見つかりませんでした".into(),
                true,
            )
        })?;

    let client = reqwest::blocking::Client::builder()
        .timeout(StdDuration::from_secs(10))
        .user_agent("ai-dashboard/0.1.0")
        .build()
        .map_err(|error| {
            (
                format!("HTTPクライアントを作成できませんでした: {error}"),
                false,
            )
        })?;
    let response = client
        .get("https://api.anthropic.com/api/oauth/usage")
        .bearer_auth(access_token)
        .header("anthropic-beta", "oauth-2025-04-20")
        .send()
        .map_err(|error| {
            (
                format!("Claude使用率APIへ接続できませんでした: {error}"),
                false,
            )
        })?;
    let status = response.status();
    if !status.is_success() {
        return Err(if status == reqwest::StatusCode::UNAUTHORIZED {
            (
                "Claude OAuth認証の有効期限が切れています。Claude Codeで再ログインしてください。"
                    .into(),
                true,
            )
        } else {
            (format!("Claude使用率APIがHTTP {status}を返しました"), false)
        });
    }
    let usage: Value = response.json().map_err(|error| {
        (
            format!("Claude使用率APIの応答を解析できませんでした: {error}"),
            false,
        )
    })?;
    Ok((
        parse_claude_limit(&usage, "five_hour"),
        parse_claude_limit(&usage, "seven_day"),
    ))
}

fn collect_dashboard_data(bucket_count: usize) -> Result<DashboardData, String> {
    let count = bucket_count.clamp(1, 90);
    let home = dirs::home_dir().ok_or("ホームディレクトリを取得できませんでした")?;
    let now = Utc::now();
    let codex = parse_codex(&home.join(".codex").join("sessions"), now, count);
    let claude_buckets = parse_claude(&home.join(".claude").join("projects"), now, count);
    let mut warnings = Vec::new();
    let mut claude_login_required = false;
    let (five_hour, weekly) = match read_claude_limits(&home) {
        Ok(limits) => limits,
        Err((error, login_required)) => {
            claude_login_required = login_required;
            warnings.push(error);
            (None, None)
        }
    };
    Ok(DashboardData {
        codex,
        claude: ToolUsage {
            five_hour,
            weekly,
            buckets: claude_buckets,
        },
        claude_login_required,
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

#[tauri::command]
async fn login_claude() -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;

        const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
        tauri::async_runtime::spawn_blocking(|| {
            let status = Command::new("powershell.exe")
                .args(["-NoProfile", "-Command", "claude auth login"])
                .creation_flags(CREATE_NEW_CONSOLE)
                .status()
                .map_err(|error| {
                    format!("Claudeログイン用ターミナルを開けませんでした: {error}")
                })?;
            if status.success() {
                Ok(())
            } else {
                Err(format!(
                    "Claudeログインが終了コード {status} で終了しました"
                ))
            }
        })
        .await
        .map_err(|error| format!("Claudeログイン処理に失敗しました: {error}"))?
    }

    #[cfg(target_os = "macos")]
    {
        tauri::async_runtime::spawn_blocking(|| {
            let status = Command::new("osascript")
                .args([
                    "-e",
                    "tell application \"Terminal\" to do script \"claude auth login\"",
                ])
                .status()
                .map_err(|error| {
                    format!("Claudeログイン用ターミナルを開けませんでした: {error}")
                })?;
            if status.success() {
                Ok(())
            } else {
                Err(format!(
                    "Claudeログイン用ターミナルの起動に失敗しました（終了コード {status}）"
                ))
            }
        })
        .await
        .map_err(|error| format!("Claudeログイン処理に失敗しました: {error}"))?
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        tauri::async_runtime::spawn_blocking(|| {
            const CANDIDATES: &[(&str, &[&str])] = &[
                ("x-terminal-emulator", &["-e", "claude auth login"]),
                ("gnome-terminal", &["--", "bash", "-lc", "claude auth login"]),
                ("konsole", &["-e", "bash", "-lc", "claude auth login"]),
                (
                    "xfce4-terminal",
                    &["-x", "bash", "-lc", "claude auth login"],
                ),
                ("xterm", &["-e", "bash", "-lc", "claude auth login"]),
            ];
            for (program, args) in CANDIDATES {
                match Command::new(program).args(*args).status() {
                    Ok(status) if status.success() => return Ok(()),
                    Ok(status) => {
                        return Err(format!(
                            "Claudeログインが終了コード {status} で終了しました"
                        ));
                    }
                    Err(_) => continue,
                }
            }
            Err("利用可能なターミナルエミュレータが見つかりませんでした".into())
        })
        .await
        .map_err(|error| format!("Claudeログイン処理に失敗しました: {error}"))?
    }

    #[cfg(not(any(windows, unix)))]
    Err("Claudeログイン用ターミナルの起動はこのOSでは対応していません".into())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default();
    #[cfg(debug_assertions)]
    let builder = builder.plugin(
        tauri_plugin_mcp_bridge::Builder::new()
            .bind_address("127.0.0.1")
            .build(),
    );
    builder
        .invoke_handler(tauri::generate_handler![get_dashboard_data, login_claude])
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
    fn parses_claude_oauth_limits() {
        let usage = serde_json::json!({
            "five_hour": {"utilization": 35.0, "resets_at": "2026-08-30T08:50:00+09:00"},
            "seven_day": {"utilization": 99.0, "resets_at": "2026-08-31T00:00:00+09:00"}
        });
        let five = parse_claude_limit(&usage, "five_hour").unwrap();
        let weekly = parse_claude_limit(&usage, "seven_day").unwrap();
        assert_eq!(five.used_percent, 35.0);
        assert_eq!(five.resets_at, Some(1_788_047_400));
        assert_eq!(weekly.used_percent, 99.0);
        assert_eq!(weekly.resets_at, Some(1_788_102_000));
    }
}
