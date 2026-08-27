# Claude Code / Codex CLI 使用量可視化 — 検討経緯

## 目的

Claude Code と Codex CLI のトークン使用量・消費推移を、時間軸で可視化したい。
当初は自宅のRaspberry PiでホスティングしているGrafana（SQLiteプラグイン導入済み）で、自宅LAN内でだけ見られればよい、という前提だった。

## 検討した方式と却下理由

### 1. JSONLログの直接パース（却下）

- Claude Codeは `~/.claude/projects/**/*.jsonl` の各アシスタントメッセージに `timestamp` と `usage`（input_tokens, output_tokens, cache_creation_input_tokens, cache_read_input_tokens）、`model` が記録されている（実機で確認済み）。
- Codex CLIは `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl` に `type:"token_count"` イベントがあり、`timestamp`・累積(`total_token_usage`)・差分(`last_token_usage`)・`rate_limits` が記録されている（実機で確認済み）。
- ログ自体は各マシンのローカルファイルにしかないため、ラズパイ側から読むには以下のいずれかが必要:
  - **SMB/SSHでラズパイ側からpull**: Windowsデスクトップに加えてUbuntuラップトップ（外出先で使用、常時オンラインではない）もあるため、オフライン時のデータ欠損・マシン増減への対応の煩雑さから不採用。
  - **各マシンに送信スクリプトを常駐させてpush**: ユーザーが明確に「他デバイスへの追加スクリプト導入はしたくない」と却下。

### 2. `/usage` スラッシュコマンド（却下）

- Claude Code CLIの `/usage` は、その時点のプラン消費率の**スナップショット**（直近24時間/7日間を切替可能）であり、時系列データではない。
- 非対話的に取得するCLIサブコマンドやJSON出力オプションは公式に存在しない。
- 参照: claude-code-guideサブエージェントによる調査結果。

### 3. claude.ai / chatgpt.com の内部API定期アクセス（却下）

- ブラウザで両サービスの使用量設定ページを開き、ネットワークリクエストを確認したところ、以下の非公開APIが裏で叩かれていることを確認:
  - Claude: `https://claude.ai/api/organizations/{org_id}/usage` (GET)
  - ChatGPT: `https://chatgpt.com/backend-api/wham/usage` (GET)
- どちらもブラウザのログインセッションCookie認証で、ドキュメント化されていない内部API。
- 利用規約を実際に確認した結果、**両社とも自動化アクセスを明確に禁止**していることが判明:
  - Anthropic消費者向け利用規約 Section 3: 「bot、スクリプト等の自動化・非人間的手段」でのサービス利用を、Anthropic APIキー使用時や明示的許可がある場合を除き禁止。「crawl, scrape, or otherwise harvest data」も禁止。
  - OpenAI利用規約（禁止事項）: 「データ又はアウトプットを自動又はプログラムにより引き出すこと」を明確に禁止。
- このため、自分のアカウントの自分の使用量取得であっても不採用と判断。

なお調査の過程で、ChatGPT側の使用状況ページ「残り88%」「残り98%」は**残存率**、Claude.ai側の「43%使用済み」は**消費率**であることが判明し、表記の向きが逆であることが分かった（ユーザーが「Codexの使用量が多い」と感じたのは、この表記の見間違いだった）。

### 4. 自宅ラズパイをOTLPエンドポイントにする方式（却下）

- Claude Code、Codex CLIともに公式のOpenTelemetry (OTel) export機能を持ち、環境変数/設定ファイルのみで有効化できる（常駐スクリプト不要）。
- ただし外出先のUbuntuラップトップから自宅LAN内のラズパイへは直接到達できないという疎通性の問題が発覚。
- VPN（Tailscaleなど）で解決可能だが、「自宅LAN内でだけ見られればよい」という前提から踏み出すことになるため保留。

## 現在の方針（採用）

「自宅のGrafanaで見ることにはこだわらない」という要件緩和を受け、**Grafana Cloud（無料枠）を中継点にする**方式に決定。詳細は `docs/design.md` を参照。

## 未確定・要確認事項

- Grafana Cloudのアカウント作成（ユーザー自身によるサインアップが必要、メール登録のみでカード登録は不要）
- OTLPエンドポイントURL・APIキーの取得
- Claude Codeの `claude_code.token.usage` メトリクスの分解粒度（input/output別に取れるかは未確認）
- Ubuntuラップトップ側の設定作業の進め方（手順書を渡すか、SSH等でこちらが代行するか）
- Grafana Cloud無料枠は超過時に自動課金される仕組みのため、使用量アラート設定の要否
