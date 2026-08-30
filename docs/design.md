# Claude Code / Codex CLI 使用量可視化 — 設計

## 全体構成

```
各マシン（Windowsデスクトップ、Ubuntuラップトップ／自宅・外出先問わず）
  └─ Claude Code CLI ─┐
  └─ Codex CLI      ─┤ OTel export（環境変数 / config.tomlの設定のみ、常駐スクリプト不要）
                       ↓ OTLP (push)
                   Grafana Cloud（無料枠）
                       - OTLPエンドポインドでメトリクス受信
                       - Prometheus/Mimir互換ストレージに保存
                       ↓
                   Grafana Cloud上のダッシュボードで可視化
```

- データの収集・保管・可視化をすべてGrafana Cloudに任せることで、自宅ラズパイのインフラ（Collector, Prometheus等）構築が不要になり、外出先のラップトップからの疎通性問題も解消される。
- 生のトークン数ではなく、プラン消費率（%）が見られれば十分という要件（ユーザー確認済み）。

## 各ツールのOTel export設定

Windowsデスクトップの接続情報:

- OTLP endpoint: `https://otlp-gateway-prod-ap-northeast-0.grafana.net/otlp`
- Instance ID: `1810536`
- 認証情報はユーザー環境変数に保存し、リポジトリには記録しない。

### Claude Code CLI（環境変数ベース）

| 環境変数 | 用途 |
|---|---|
| `CLAUDE_CODE_ENABLE_TELEMETRY=1` | OTel export自体の有効化 |
| `OTEL_EXPORTER_OTLP_ENDPOINT` | Grafana CloudのOTLPエンドポイントURL |
| `OTEL_EXPORTER_OTLP_PROTOCOL` | `grpc` または `http/protobuf` |
| `OTEL_EXPORTER_OTLP_HEADERS` | 認証ヘッダー（例: `Authorization=Basic <Grafana CloudのAPIキー>`） |
| `OTEL_METRICS_EXPORTER=otlp` | メトリクスのエクスポータ種別 |
| `OTEL_METRIC_EXPORT_INTERVAL` | 送信間隔（デフォルト60000ms） |
| `OTEL_LOGS_EXPORTER=none` | プロンプト等を含み得るログ送信を無効化 |

- 主なメトリクス: `claude_code.token.usage`、`claude_code.cost.usage`（USD）、`claude_code.session.count` など。`session.id` / `user.email` 等のラベルが自動付与される。
- プロンプト内容は**デフォルトで送信されない**（redacted）。プライバシー面で追加対応は不要。
- 公式ドキュメント: https://code.claude.com/docs/en/monitoring-usage.md

### Codex CLI（`~/.codex/config.toml` の `[otel]` セクション）

- Codex CLI 0.150.1では `runtime_metrics` を有効化してもGrafana Cloudに `target_info` しか保存されなかったため、構造化OTelログ方式を採用。
- ログ用の `exporter` をOTLP HTTPの `/v1/logs` に設定し、`metrics_exporter` は `none` にする。
- `log_user_prompt = false` を維持してプロンプト本文を送信しない。ログには実行結果やツール利用情報などが含まれる可能性がある。
- `codex.sse_event` の `response.completed` に含まれるtoken数をLokiで集計し、ダッシュボードに利用する。
- 設定例: `exporter = { otlp-http = { endpoint="<Grafana CloudのOTLPエンドポイント>/v1/logs", protocol="binary", headers={"Authorization"="Basic ${GRAFANA_CLOUD_OTLP_AUTH}"} } }`
- 認証値は環境変数など任意の名前で参照する形（Codex側に専用の環境変数名は無い）。
- 公式ドキュメント: https://developers.openai.com/codex/config-advanced 、https://developers.openai.com/codex/config-reference

## Grafana Cloud 無料枠の仕様（2026-08-28時点、公式pricingページで確認）

| 項目 | 内容 |
|---|---|
| メトリクス保持期間 | 14日間 |
| メトリクス上限 | 月10,000 active series まで |
| ログ | 月50GB取り込みまで、14日保持 |
| トレース | 月50GB取り込みまで、14日保持 |
| Grafana利用者数 | 月3ユーザーまで |
| クレジットカード登録 | 不要（サインアップ時） |
| 超過時の扱い | 自動的に従量課金へ移行（データ破棄ではない） |

- 本用途（メトリクス数個〜十数種類、10分間隔程度の送信、プロンプト非送信）であれば無料枠の範囲に収まる想定。
- 念のため、Grafana Cloud側の使用量アラート設定を検討する。

## 実装ステップ（未着手）

1. Grafana Cloud無料アカウントのサインアップ（ユーザー自身）
2. OTLPエンドポイントURL・APIキーの取得（完了）
3. Windowsデスクトップの環境変数設定（完了。Codexは構造化ログ送信実行済み・Cloud側確認待ち、Claudeは再ログイン待ち）
4. Ubuntuラップトップの環境変数/config.toml設定
5. Grafana Cloud上でのダッシュボード作成（トークン使用率%の時系列パネル）

## 経緯・却下した代替案

`docs/progress.md` を参照。

## ローカルアプリへの方針変更（2026-08-30）

最終的に必要な表示を、Grafana Cloudではなくローカルデスクトップアプリとして実装する。

### 画面構成

1. **現在の使用上限率**
   - CodexとClaudeをそれぞれ表示する。
   - 各ツールについて、5時間枠と週次枠の使用済み割合を円形ゲージ（ドーナツチャート等）で表示する。
   - 使用済み割合、残り割合、リセット日時を併記する。
2. **直近5時間のトークン使用量の時間推移**
   - CodexとClaudeのローカルJSONLを読み、直近5時間のトークン使用量を10分単位のバケットへ集計して時系列グラフにする。
   - input/output/cache/reasoning等の内訳を保持し、合計と内訳を切り替えまたは積み上げ表示できるようにする。
   - これは契約上限率の履歴ではなく、JSONLから得た実トークン量の推移である。

### データ取得元

| 表示対象 | Codex CLI | Claude Code |
|---|---|---|
| 現在の5時間枠・週次枠 | JSONLの最新 `rate_limits` | Claude Codeと同じ内部OAuth使用率API |
| 直近5時間の10分ごとのトークン推移 | JSONLの `timestamp` と `last_token_usage` | JSONLの `timestamp` と `message.usage` |

- Codexの `total_token_usage` はセッション内累積なので、時間推移には差分値の `last_token_usage` を使用する。
- Claudeは同一API応答が複数行に記録される場合があるため、`message.id`、`requestId`、usage内容などで重複排除する。
- 直近5時間のトークン推移は、単一デバイス上のローカルログだけを対象とする。別デバイスでの使用によるアカウント全体との差は許容し、同期・集約機能は作らない。
- 現在の上限率はサーバーから各CLIへ返された値を使うため、トークン数から推定しない。Claude側のOAuth APIは非公開仕様のため、失敗時は警告を表示して空欄とする。
- データは起動時と10分ごとに自動更新し、更新ボタンからも手動取得できるようにする。
