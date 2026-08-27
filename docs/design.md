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

### Claude Code CLI（環境変数ベース）

| 環境変数 | 用途 |
|---|---|
| `CLAUDE_CODE_ENABLE_TELEMETRY=1` | OTel export自体の有効化 |
| `OTEL_EXPORTER_OTLP_ENDPOINT` | Grafana CloudのOTLPエンドポイントURL |
| `OTEL_EXPORTER_OTLP_PROTOCOL` | `grpc` または `http/protobuf` |
| `OTEL_EXPORTER_OTLP_HEADERS` | 認証ヘッダー（例: `Authorization=Basic <Grafana CloudのAPIキー>`） |
| `OTEL_METRICS_EXPORTER=otlp` | メトリクスのエクスポータ種別 |
| `OTEL_METRIC_EXPORT_INTERVAL` | 送信間隔（デフォルト60000ms） |

- 主なメトリクス: `claude_code.token.usage`、`claude_code.cost.usage`（USD）、`claude_code.session.count` など。`session.id` / `user.email` 等のラベルが自動付与される。
- プロンプト内容は**デフォルトで送信されない**（redacted）。プライバシー面で追加対応は不要。
- 公式ドキュメント: https://code.claude.com/docs/en/monitoring-usage.md

### Codex CLI（`~/.codex/config.toml` の `[otel]` セクション）

- `metrics_exporter` にOTLP HTTPエンドポイントと認証ヘッダーを設定。
- 例のイメージ: `exporter = { otlp-http = { endpoint="<Grafana CloudのOTLPエンドポイント>/v1/logs", protocol="binary", headers={"Authorization"="Bearer ${OTLP_TOKEN}"} } }`
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
2. OTLPエンドポイントURL・APIキーの取得
3. Windowsデスクトップの環境変数設定
4. Ubuntuラップトップの環境変数/config.toml設定
5. Grafana Cloud上でのダッシュボード作成（トークン使用率%の時系列パネル）

## 経緯・却下した代替案

`docs/progress.md` を参照。
