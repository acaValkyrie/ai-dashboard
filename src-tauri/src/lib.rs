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
struct AntigravityGroup {
    name: String,
    five_hour: Option<RateLimit>,
    weekly: Option<RateLimit>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AntigravityUsage {
    groups: Vec<AntigravityGroup>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DashboardData {
    codex: ToolUsage,
    claude: ToolUsage,
    /// Antigravity CLI(`agy`)が見つからない場合は`None`。
    antigravity: Option<AntigravityUsage>,
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

/// Claude Codeの認証情報を読み込む。
///
/// Windows/Linuxでは`~/.claude/.credentials.json`に保存されるが、
/// macOSではKeychainのサービス名`Claude Code-credentials`に同じJSONが保存される。
/// ファイルがあればそれを優先し、無ければmacOSに限りKeychainへフォールバックする。
fn load_claude_credentials(home: &Path) -> Result<Value, (String, bool)> {
    let credentials_path = home.join(".claude").join(".credentials.json");
    match File::open(&credentials_path) {
        Ok(file) => serde_json::from_reader(file).map_err(|error| {
            (
                format!("Claude認証情報を解析できませんでした: {error}"),
                true,
            )
        }),
        Err(file_error) => {
            #[cfg(target_os = "macos")]
            {
                if let Some(raw) = read_claude_credentials_from_keychain() {
                    return serde_json::from_str(&raw).map_err(|error| {
                        (
                            format!("Keychain内のClaude認証情報を解析できませんでした: {error}"),
                            true,
                        )
                    });
                }
                return Err((
                    format!(
                        "Claude認証情報が見つかりませんでした(ファイル: {file_error} / Keychain: 見つからず)。Claude Codeでログインしてください。"
                    ),
                    true,
                ));
            }
            #[cfg(not(target_os = "macos"))]
            Err((
                format!("Claude認証情報を開けませんでした: {file_error}"),
                true,
            ))
        }
    }
}

/// macOSのKeychainからClaude Codeの認証情報JSONを取得する。
#[cfg(target_os = "macos")]
fn read_claude_credentials_from_keychain() -> Option<String> {
    let output = Command::new("security")
        .args(["find-generic-password", "-s", "Claude Code-credentials", "-w"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let raw = String::from_utf8(output.stdout).ok()?;
    let raw = raw.trim();
    if raw.is_empty() {
        None
    } else {
        Some(raw.to_owned())
    }
}

fn read_claude_limits(
    home: &Path,
) -> Result<(Option<RateLimit>, Option<RateLimit>), (String, bool)> {
    let credentials = load_claude_credentials(home)?;
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

/// Antigravity CLI(`agy`)の実行ファイルを探す。
///
/// GUIアプリから起動されるとシェルのPATHが引き継がれないことがあるため、
/// PATHに加えて既定のインストール先(`~/.local/bin`)も候補に含める。
fn find_antigravity_cli(home: &Path) -> Option<PathBuf> {
    let names: &[&str] = if cfg!(windows) {
        &["agy.exe", "agy.cmd", "agy"]
    } else {
        &["agy"]
    };
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).collect())
        .unwrap_or_default();
    dirs.push(home.join(".local").join("bin"));
    dirs.into_iter()
        .flat_map(|dir| names.iter().map(move |name| dir.join(name)))
        .find(|candidate| candidate.is_file())
}

/// `agy -p /quota --output-format json` の出力から上限率を取り出す。
///
/// 出力は `command.data.groups[]` にモデルグループ(Gemini / Claude+GPT)が並び、
/// 各グループの `buckets[]` に `window`("5h" | "weekly")、`remaining_fraction`、
/// `reset_time`(RFC3339)が入っている。
fn parse_antigravity_quota(value: &Value) -> Vec<AntigravityGroup> {
    let Some(groups) = value
        .pointer("/command/data/groups")
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };
    groups
        .iter()
        .filter_map(|group| {
            let name = group.get("name")?.as_str()?.to_owned();
            let mut five_hour = None;
            let mut weekly = None;
            for bucket in group
                .get("buckets")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let Some(remaining) = bucket.get("remaining_fraction").and_then(Value::as_f64)
                else {
                    continue;
                };
                let limit = RateLimit {
                    used_percent: ((1.0 - remaining) * 100.0).clamp(0.0, 100.0),
                    resets_at: bucket
                        .get("reset_time")
                        .and_then(Value::as_str)
                        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
                        .map(|value| value.timestamp()),
                    source: "Antigravity CLI /quota".into(),
                };
                match bucket.get("window").and_then(Value::as_str) {
                    Some("5h") => five_hour = Some(limit),
                    Some("weekly") => weekly = Some(limit),
                    _ => {}
                }
            }
            Some(AntigravityGroup {
                name,
                five_hour,
                weekly,
            })
        })
        .collect()
}

fn read_antigravity_limits(cli: &Path) -> Result<Vec<AntigravityGroup>, String> {
    let mut command = Command::new(cli);
    command.args(["-p", "/quota", "--output-format", "json"]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let output = command
        .output()
        .map_err(|error| format!("Antigravity CLIを実行できませんでした: {error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let detail = stderr.lines().last().unwrap_or("").trim();
        return Err(format!(
            "Antigravity CLIが上限率を返しませんでした({}): {detail}",
            output.status
        ));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    // 前後にログ行が混ざる可能性に備え、JSONオブジェクトの行だけを探す。
    let json_line = stdout
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with('{'))
        .ok_or_else(|| "Antigravity CLIの出力にJSONが含まれていませんでした".to_owned())?;
    let value: Value = serde_json::from_str(json_line)
        .map_err(|error| format!("Antigravity CLIの出力を解析できませんでした: {error}"))?;
    if value.get("status").and_then(Value::as_str) != Some("SUCCESS") {
        return Err(format!(
            "Antigravity CLIがエラーを返しました: {}",
            value
                .get("response")
                .and_then(Value::as_str)
                .unwrap_or("詳細不明")
                .trim()
        ));
    }
    Ok(parse_antigravity_quota(&value))
}

fn collect_dashboard_data(bucket_count: usize) -> Result<DashboardData, String> {
    let count = bucket_count.clamp(1, 90);
    let home = dirs::home_dir().ok_or("ホームディレクトリを取得できませんでした")?;
    let now = Utc::now();
    // Antigravity CLIは応答に数秒かかるため、他の集計と並行して実行する。
    let antigravity_task = std::thread::spawn({
        let home = home.clone();
        move || find_antigravity_cli(&home).map(|cli| read_antigravity_limits(&cli))
    });
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
    let antigravity = match antigravity_task.join() {
        Ok(None) => None,
        Ok(Some(Ok(groups))) => Some(AntigravityUsage { groups }),
        Ok(Some(Err(error))) => {
            warnings.push(error);
            Some(AntigravityUsage { groups: Vec::new() })
        }
        Err(_) => {
            warnings.push("Antigravityの上限率取得処理が異常終了しました".into());
            None
        }
    };
    Ok(DashboardData {
        codex,
        claude: ToolUsage {
            five_hour,
            weekly,
            buckets: claude_buckets,
        },
        antigravity,
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
    fn parses_antigravity_quota_groups() {
        let output = serde_json::json!({
            "status": "SUCCESS",
            "command": {"name": "usage", "data": {"groups": [
                {"name": "Gemini Models", "buckets": [
                    {"id": "gemini-weekly", "window": "weekly", "remaining_fraction": 0.25, "reset_time": "2026-09-09T14:09:30Z"},
                    {"id": "gemini-5h", "window": "5h", "remaining_fraction": 1, "reset_time": "2026-09-02T19:09:30Z"}
                ]},
                {"name": "Claude and GPT models", "buckets": [
                    {"id": "3p-weekly", "window": "weekly", "remaining_fraction": 0.0}
                ]}
            ]}}
        });
        let groups = parse_antigravity_quota(&output);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].name, "Gemini Models");
        let weekly = groups[0].weekly.as_ref().unwrap();
        assert_eq!(weekly.used_percent, 75.0);
        assert_eq!(weekly.resets_at, Some(1_788_962_970));
        assert_eq!(groups[0].five_hour.as_ref().unwrap().used_percent, 0.0);
        assert_eq!(groups[1].weekly.as_ref().unwrap().used_percent, 100.0);
        assert!(groups[1].weekly.as_ref().unwrap().resets_at.is_none());
        assert!(groups[1].five_hour.is_none());
        assert!(parse_antigravity_quota(&serde_json::json!({})).is_empty());
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
