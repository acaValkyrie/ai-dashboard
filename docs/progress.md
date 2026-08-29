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

## 実装状況（2026-08-28）

- Grafana CloudアカウントとStackを作成済み。
- Windowsデスクトップ用のOTLP endpointとInstance IDを取得済み。
- Grafana Cloudへの認証疎通を確認済み（OTLP metrics endpointがHTTP 200を返却）。
- Claude Code用のユーザー環境変数を設定済み。メトリクスのみ有効にし、ログ送信は無効化。
- Codex CLIの `~/.codex/config.toml` にメトリクスexport設定を追加済み。ログ送信は無効化。
- Codex CLI 0.150.1のmetrics exporterは `target_info` しか保存されなかったため、構造化OTelログ方式へ切り替え済み。`log_user_prompt = false` でプロンプト本文は無効。Grafana Cloud側でのログ受信確認待ち。
- Claude CodeはOAuthセッション期限切れのため実送信テスト未完了。再ログイン後に確認する。
- ダッシュボード作成は未実施。

## 未確定・要確認事項

- Claude Codeの `claude_code.token.usage` メトリクスのGrafana Cloud上での受信確認
- Codex CLIの `codex.sse_event` 構造化ログとtokenフィールドのGrafana Cloud上での受信確認
- Ubuntuラップトップ側の設定作業の進め方（手順書を渡すか、SSH等でこちらが代行するか）
- Grafana Cloud無料枠は超過時に自動課金される仕組みのため、使用量アラート設定の要否

## Codex → Grafana Cloud送信の現在の症状（Claude引き継ぎ用、2026-08-28）

### 環境

- Windowsデスクトップ
- Codex CLI `0.150.1`
- Grafana Cloud OTLP endpoint: `https://otlp-gateway-prod-ap-northeast-0.grafana.net/otlp`
- Grafana Cloud Instance ID: `1810536`
- 認証Tokenはユーザー環境変数由来のBasic認証値として保持。Token本体はリポジトリへ保存していない。

### OTLP接続・認証

- `/otlp/v1/metrics` への空のprotobuf POSTはHTTP 200。
- `/otlp/v1/logs` への空のprotobuf POSTはHTTP 204。
- したがってendpoint、Basic認証、`metrics:write` / `logs:write` 権限は通っている。

### metrics方式で発生した症状

Codexの `[otel]` で `metrics_exporter` をOTLP HTTP `/v1/metrics` に設定し、Terraで複数回短い `codex exec` を実行した。Grafana CloudのMetrics Drilldownには `target_info` だけが現れ、`codex`、`turn_token_usage`、`token_usage`、`api_request`、`tool_call` 等の実行メトリクスは見つからなかった。

- `target_info` のcountは最初1、再試験後2になったため、Codexプロセスからresource情報自体は到達している。
- `codex features list` では `runtime_metrics` が `under development / false` だった。
- `[features] runtime_metrics = true` を追加して再試験したが、結果は `target_info` のみ。
- `OTEL_METRIC_EXPORT_INTERVAL=5000` を設定して5秒間隔でも再試験したが変化なし。
- 60秒超のテストも試みたが、Codex内からの `Start-Sleep` はWindows二重サンドボックスの `CreateProcessAsUserW failed` で失敗。対話TTYの起動も同系統のプロセス生成エラーで実行できなかった。
- 短いプロセス終了時のmetrics flush問題、Codex 0.150.1の試験的metrics exporter実装、またはGrafana OTLP変換との相性が候補。

### 現在の設定と次の確認

metrics方式をいったん停止し、Codexの正式な構造化OTelログ方式へ切り替えた。

- `exporter`: OTLP HTTP `/v1/logs`
- `metrics_exporter = "none"`
- `log_user_prompt = false`
- `[features] runtime_metrics = false`
- Terra指定の短い送信テスト `OTEL_LOG_TEST_OK` は実行済み。
- OpenAI公式ドキュメント上、`codex.sse_event` の `response.completed` にはtoken countsが含まれるため、Loki上で抽出・集計する想定。
- 未確認: Grafana CloudのLogs Drilldownに `codex` / `codex_cli` サービスまたは `codex.sse_event` が届いているか。
- このCodexセッションからログイン済みChromeを直接操作するbrowser/computer-use toolは利用できないため、ユーザーによる画面確認またはClaude Code側からのブラウザ操作が必要。
- 補足: Claude Code CLI（このセッション）にもブラウザ操作系ツールは搭載されていない（Claude for Chrome拡張機能とは別プロダクト）。Grafana CloudのクエリAPIを直接叩く代替案も検討したが、クエリ用エンドポイントURLの推測は避けるべきため保留し、結局ユーザーがChromeで画面確認する方式を採用した。

## Logs Drilldownの確認結果（2026-08-28）

- ユーザーがGrafana CloudのMetrics Drilldown（`grafanacloud-heartykelp2201-prom`データソース、直近15分）を画面確認。
- 「All metrics」で表示されたのは `target_info` のみ（count 1→2の推移）。`codex` / `token_usage` 系の実行メトリクスは存在せず、上記「metrics方式で発生した症状」の記録と一致することを視覚的にも確認。

## 切り分け方針（ユーザー提案、2026-08-28）

Claude Code側でも同じGrafana Cloudエンドポイントに向けてOTel送信を試し、
- Claude Codeでも届かない → Grafana Cloud側（設定・エンドポイント等）を疑う
- Claude Codeは届く → Codex CLI固有の問題と判断

という切り分けを行う方針にした。

### 調査で判明した点：現在のセッションではまだテストできない

`/login`で再ログインした直後にClaude Code側のOTel環境変数を確認したところ、以下が判明した。

- **永続化されたユーザー環境変数（レジストリ）には設定済みの値がすべて正しく入っている**: `OTEL_EXPORTER_OTLP_ENDPOINT=https://otlp-gateway-prod-ap-northeast-0.grafana.net/otlp`、`OTEL_EXPORTER_OTLP_PROTOCOL=http/protobuf`、`OTEL_EXPORTER_OTLP_HEADERS=Authorization=Basic <値>`、`OTEL_METRICS_EXPORTER=otlp`、`OTEL_METRIC_EXPORT_INTERVAL=5000`、`OTEL_LOGS_EXPORTER=none`、`CLAUDE_CODE_ENABLE_TELEMETRY=1`。
- **しかし今動いているClaude Codeプロセス自身の環境変数には`CLAUDE_CODE_ENABLE_TELEMETRY=1`しか載っておらず、`OTEL_EXPORTER_OTLP_*`系が反映されていない**。Windowsのユーザー環境変数は、値を設定した後に新しいプロセスを起動しないと既存プロセスには反映されない仕様のため、このセッションのプロセスはOTel設定が完了する前に起動していたと考えられる。
- したがって、このセッション上ではClaude Code側のOTel送信テストはまだ実施不可能。**Claude Code CLIを一度完全に終了して開き直す**ことで新しい環境変数がプロセスに反映されるはずなので、次回はそこから再開する。

### 次にやること

1. Claude Code CLIセッションを再起動（ユーザー作業）。
2. 再起動後のセッションでいくつかやり取りしてトークン使用を発生させる。
3. Grafana CloudのMetrics Drilldownで`claude_code.token.usage`等が届いているか確認。
4. 届けば「Codex CLI固有の問題」、届かなければ「Grafana Cloud側の問題」の可能性が高いと判断し、次の調査に進む。

### 再確認（2026-08-28、別セッションで再開）

「Claude Code CLIセッションを再起動」しただけの後継セッションでも、プロセスの環境変数は`CLAUDE_CODE_ENABLE_TELEMETRY=1`のみで、`OTEL_EXPORTER_OTLP_*`系は依然反映されていなかった（レジストリ側の値は正しいことを再確認済み）。

Windowsのユーザー環境変数はプロセスの起動時にその時点の値がスナップショットされ、以降は子プロセスがいくら起動し直されても、**祖先プロセス（親のターミナルアプリ）が環境変数設定より前から起動していれば**反映されない。したがって「Claude Code CLIプロセスの再起動」では不十分で、**ターミナルアプリ（PowerShell/Windows Terminal等）自体を完全に終了して開き直す**必要があると判断した。

次にやること（更新）:
1. ターミナルアプリ自体を完全に終了し、開き直す（ユーザー作業）。
2. 新しいターミナルでClaude Codeを起動し、プロセスの環境変数に`OTEL_EXPORTER_OTLP_*`系が反映されているか確認。
3. 反映されていれば、いくつかやり取りしてトークン使用を発生させ、Grafana CloudのMetrics Drilldownで`claude_code.token.usage`等が届いているか確認。
4. 届けば「Codex CLI固有の問題」、届かなければ「Grafana Cloud側の問題」の可能性が高いと判断し、次の調査に進む。

## 根本原因判明：delta temporalityがGrafana Cloud（Mimir）に拒否されていた（2026-08-28）

### 経緯

ユーザー環境変数を反映させた新しいターミナルでClaude Codeを起動しても、Grafana Cloud側にメトリクスが届かなかった（「でてないね」）。ここで「ターミナルアプリの再起動」自体が本当に効いているかを疑い、以下の手法に切り替えた。

- `wt -d .`で新規プロセスとしてWindows Terminalを起動し、`Get-Process -Name WindowsTerminal`で実際にプロセスが直近数十秒以内に起動されたことを確認（古いプロセスの残存はなかった）。
- それでも解決しなかったため、「ターミナル再起動待ち」ではなく、レジストリ(`HKCU:\Environment`)から`OTEL_*`系を読み込んで**その場のシェルの`$env:`に直接注入してから`claude`を起動する**方式（bashの`source`相当）に切り替えた。祖先プロセスの起動タイミングに一切依存しないため、以後はこの方式を標準の確認手順とする。

### 診断で判明したこと

Claude Code CLIには`--debug <category>`（例: `--debug otel`）と`--debug-file <path>`があり、これを使うと`claude -p "..."`（非対話モード）だけでOTel送信の成否をファイルに出力できる。これにより、Claude Code自身のプロセスの中で何が起きているかを直接確認できた。

デバッグログに以下が出力されていた：

```
[3P telemetry] First metrics export: FAILED (Bad Request)
[ERROR] [3P telemetry] OTEL diag error: {"message":"PeriodicExportingMetricReader: metrics export failed (error OTLPExporterError: Bad Request)", ...}
```

つまり、疎通・認証（空POSTがHTTP 200/204）は問題なかったが、**実際のメトリクスデータを含むPOSTがGrafana Cloud側からHTTP 400で拒否されていた**。

Web調査の結果、Grafana Cloud（Mimir）のOTLPメトリクスエンドポイントは**cumulative temporalityしか受け付けず、delta temporalityは`400: invalid temporality and type combination for metric`で拒否する**という既知の制限があることが判明（[grafana/mimir#6696](https://github.com/grafana/mimir/issues/6696)、[grafana/mimir#10439](https://github.com/grafana/mimir/issues/10439)）。Claude Code CLIのOTel SDKはデフォルトでdelta temporalityを使っていたため、これに該当していた。

### 対処と確認

標準のOTel環境変数`OTEL_EXPORTER_OTLP_METRICS_TEMPORALITY_PREFERENCE=cumulative`を追加したところ、同じ`--debug otel`テストで

```
[3P telemetry] First metrics export: SUCCESS
```

に変化した。この変数をユーザー環境変数として永続化済み（`[System.Environment]::SetEnvironmentVariable(..., 'User')`）。

### 未確認・次にやること

- Grafana CloudのMetrics Drilldownで`claude_code.token.usage`等が実際にダッシュボードへ表示されるかの視覚確認（ユーザー作業、ブラウザ操作ツール未搭載のため）。
- Codex CLIは現状ログ方式（`metrics_exporter = "none"`）に切り替え済みで、この問題の影響を受けない。ただし今回delta temporalityが原因と判明したことで、Codex側もmetrics方式に戻して同様の温度設定を試す価値があるかもしれない（Codex側にtemporality preferenceを設定する手段があるか未調査）。ログ方式のままにするかmetrics方式に戻すかはユーザーと相談して決める。
- 新しいターミナントでは今後、レジストリから`$env:`への直接注入（source）方式を標準の確認手順とする。

## 解決：「No data」の正体はrate()クエリの見かけ上の問題だった（2026-08-28、Claude Code自身のブラウザ操作で確認）

### 経緯

temporality修正後もGrafana CloudのMetrics Drilldownで`claude_code_session_count_total`や`claude_code_token_usage_tokens_total`が「No data」と表示され続けた。メトリクス名はカタログに登録されているのに実データがないように見えたため、Claude Code CLI自身のブラウザ操作ツール（claude-in-chrome）でGrafana Cloudを直接操作し、以下の対照実験を行った。

1. Claude Code経由ではなく、`Invoke-WebRequest`で最小限のOTLP/JSON metricsペイロード（累積temporalityのシンプルなCounter、`manual_diag_counter_total`）を手動構築してGrafana Cloudへ直接POST。レスポンスは`200`・本文`{}`（拒否なしの完全成功）。
2. それでもMetrics Drilldownのカタログ・Breakdown画面では同じく「No data」と表示された。Claude Code固有の問題ではないことが確定。
3. Grafana CloudのExplore画面でクエリを`sum(rate(manual_diag_counter_total[$__rate_interval]))`から生の`manual_diag_counter_total`（`rate()`なし、instant query）に変更したところ、`manual_diag_counter_total{job="manual-otlp-diag-test", service_name="..."} = 5`と、送信した値がそのまま表示された。
4. 同様に`claude_code_token_usage_tokens_total`を生クエリで確認したところ、複数系列（input/output/cache種別ごと）で数千〜2万台のトークン数が実際にプロットされた。

### 結論

**ingestionは完全に成功していた。**「No data」の原因は、Metrics Drilldownのカタログ／Breakdown画面がデフォルトで`sum(rate(metric[$__rate_interval]))`というクエリを使うことにあった。`rate()`は同一系列で時間的に十分離れた2点以上のサンプルが必要だが、診断用に使った`claude -p "..."`（非対話・単発）呼び出しは毎回新規プロセス（＝新規の累積カウンタ、実質1サンプルのみ）を生成するため、`rate()`が値を計算できず「No data」に見えていただけだった。実際に長時間動作しているインタラクティブセッション（5秒間隔で定期エクスポートされる本セッション自身）では、同一系列のサンプルが十分蓄積されており、`rate()`ありでも生クエリでも問題なく表示される。

### 今後の教訓

- Grafana CloudのMetrics Drilldownで「No data」が出ても、即座に送信失敗と判断しない。まず生クエリ（`rate()`等の関数を外した`instant`クエリ）で実サンプルの有無を確認すること。
- 短命な単発プロセス（`-p`ワンショット実行など）での動作確認は、累積カウンタ系メトリクスの`rate()`ベース可視化とは相性が悪い。動作確認自体は`--debug otel`のexport成否ログで十分であり、実際のダッシュボード表示確認は継続的に動くセッションで行うべき。
- 診断が行き詰まった際、ユーザーに繰り返しブラウザでの確認を依頼するより、claude-in-chromeツールでClaude Code自身がGrafana Cloudを直接操作して検証する方が速く正確だった。ブラウザ操作が必要な調査は、可能な場合は自分で行う。

### 残作業

- ダッシュボード作成（`claude_code_token_usage_tokens_total`等を使ったパネル構築）はまだ未着手。
- Codex CLI側もmetrics方式に戻すかどうかは未確定（ユーザーと相談）。
- Ubuntuラップトップ側の設定はまだ未着手。

## Codex CLI側の対応：metrics方式は不可、ログ方式で確定（2026-08-28）

### metrics方式の再検証結果

Claude Code側のtemporality修正を踏まえ、Codexの`config.toml`を一時的にmetrics方式（`metrics_exporter = { otlp-http = {...} }`、`/v1/metrics`宛）に戻して再テストした。

- `codex exec`実行自体は成功（exit 0）。
- しかしGrafana CloudのExploreで`{__name__=~"codex.*"}`を検索しても、実際の使用量メトリクス名が**一切**登録されなかった（Claude Codeのケースとは異なり、メタデータ登録すらされていない）。
- `target_info{job="codex_exec", ...}`は届いていることを確認 — Codexプロセスからのリソース情報自体はGrafana Cloudに到達している。

**結論**: 今回判明したdelta temporality問題はCodexには当てはまらない。**Codex CLI 0.150.1のmetrics exporterは、そもそも実際のトークン使用量カウンター等を実装しておらず、`target_info`相当のリソース情報しか送信しない**（以前の調査記録にある「`runtime_metrics`が`under development`」という状態と一致）。これはCodex側の既知の機能不足であり、設定側では解決不可能。したがって`config.toml`は元のログ方式（`metrics_exporter = "none"`）に戻した。

### ログ方式（`codex.sse_event`構造化ログ）の受信確認 — 成功

前回セッションで「未確認」のままだったログ方式の実データ到達を、Claude Code自身のブラウザ操作（claude-in-chrome）でGrafana CloudのLogs Explore（`grafanacloud-heartykelp2201-logs`データソース、`{service_name="codex_cli_rs"}`）を直接操作して確認した。

- ログ自体は`codex_cli_rs`サービスとして正常に届いている（起動・認証関連のログが大半）。
- フィールド一覧に`input_token_count`（出現率15%）、`output_token_count`（15%）、`cached_token_co...`、`cache_write_toke...`等、トークン使用量に関するフィールドが実在することを確認。
- 実際に値を表示したところ、`input_token_count=28759 / output_token_count=322`、`17786 / 206`、`17541 / 40`等、実データが記録されていることを確認。

**結論**: Codex CLIのログ方式は既に正常に機能しており、追加対応は不要。ダッシュボード構築時は、Codex側はLogQLで`input_token_count`・`output_token_count`等のフィールドを抽出・集計する形になる（Claude Code側はPromQLでメトリクスを直接クエリできるのに対し、Codex側はログベースの集計になるため、クエリ方法がツールごとに異なる点はダッシュボード設計時に考慮する）。

### 副次的な調査メモ：Grafana Logs Drilldownアプリの制約

Grafana CloudのLogs Drilldownアプリ（`/a/grafana-lokiexplore-app/`）はログ本文パネルがcanvasベースで描画されており、ブラウザ自動操作からのテキスト抽出・スクロールが困難だった（`resize_window`でウィンドウを大きくしても実際のスクリーンショット解像度は変わらず、約958x557に固定されていた）。素の`/explore`画面（Loki datasource, Code modeでLogQLクエリ）はDOMベースで通常にスクロール・列追加操作ができ、こちらの方が自動操作に向いていた。今後同様の調査をする際は、Drilldown系アプリよりも素のExplore画面を優先する。

## ダッシュボード作成（2026-08-28〜29）

### 概要

Grafana CloudにダッシュボードJSON（`dashboard/import`画面の「Import via dashboard JSON model」経由）を直接投入して作成した。UIをパネルごとにクリックして組み立てるより、JSONを直接書いて一括インポートする方が高速・確実だった。

ダッシュボード名: `Claude Code / Codex CLI Usage`（URL: `https://heartykelp2201.grafana.net/d/acp76z2/claude-code-codex-cli-usage`）

パネル構成:
- Claude Code - Token Usage by Type（PromQL, `type`別スタック面グラフ）
- Claude Code - Cost (USD)
- Claude Code - Sessions (selected range)（現状No data、後述）
- Claude Code - Active Time (selected range)
- Codex CLI - Token Usage (input/output)（LogQL）
- Codex CLI - Total Tokens (selected range)（LogQL）

### 判明した問題と対処：`$__rate_interval` / `$__interval` が短すぎる

インポート直後、ほぼ全パネルが「No data」または不自然に空のグラフになった。原因は、Grafanaの組み込み変数`$__rate_interval`（PromQL）・`$__interval`（LogQL）が、スクレイプ間隔のデフォルト仮定（15秒程度）から自動計算されるため非常に短く（例: 数十秒〜1分程度）、Claude Code/Codexのメトリクス・ログはターン単位でしか更新されないまばらなデータであるため、その短い窓では`increase()`や`sum_over_time()`が値を拾えないことだった。

対処として、`increase(...[$__rate_interval])`は固定`1h`、Codex側の`sum_over_time(... | unwrap ... [$__interval])`は固定`15m`に変更したところ、正常にグラフが描画された。今後同様のダッシュボードを作る際は、`$__rate_interval`/`$__interval`をまず疑い、データの実際の更新頻度に対して短すぎないか確認すること。

### 判明した問題と対処：`increase()`では絶対値しか持たないカウンタが常に0になる

「Claude Code - Sessions」パネルは`sum(increase(claude_code_session_count_total[$__range]))`で作ったが常に`0`だった。原因は、`claude_code.session.count`はセッション開始時に1回だけ値がセットされる性質のカウンタで、選択した時間範囲内で値が変化（増加）しない限り`increase()`は0を返すため。`sum(max_over_time(claude_code_session_count_total[$__range]))`に変更して対処（各プロセス＝各系列の最大値を合計することで、実際のセッション数相当を出す設計）。ただし現時点では`claude_code_session_count_total`自体に実データがまだ存在しない（生クエリでも「No data」）ため、パネル自体は今後データが溜まれば自動的に表示される想定で保留。

### 判明した問題：Grafanaの「New query editor」（実験的エディタ）の不具合

パネル編集時にデフォルトで開く新しい実験的クエリエディタ（バナー表示あり、フィードバックアンケートあり）は、クエリを編集して実行してもグラフが更新されない不具合があった（クエリ欄自体が空にリセットされることもあった）。「Back to classic」で従来のクラシックエディタに切り替えたところ問題なく動作した。今後Grafana Cloudでパネルを編集する際は、New query editorで挙動がおかしい場合はまずクラシックエディタへの切り替えを試すこと。

### 現状

主要6パネルのうち5パネル（Token Usage、Cost、Active Time、Codex Token Usage、Codex Total Tokens）は実データで正常に表示される。Sessionsパネルのみ、メトリクス自体にまだデータがないため保留。

### 残作業

- `claude_code_session_count_total`に実データが入るか、今後の利用で確認する。
- ダッシュボードのデフォルト時間範囲は「Last 3 hours」に設定済み。実運用でデータが数日分溜まってきたら、より長い範囲（例: Last 7 days）や、日次サマリー用のパネル追加を検討する。
- Ubuntuラップトップ側の設定はまだ未着手。

## 方針変更案：使用上限を表示するローカルアプリ（2026-08-30）

本当に確認したい情報は、生のトークン数だけではなく、Claude CodeとCodex CLIそれぞれの**5時間枠・週次枠の使用率、残量、リセット日時**である。Grafana Cloud中心の構成とは別に、各CLIのローカル状態を読むデスクトップアプリを有力案とする。

- Codex CLI: `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl` の `token_count.rate_limits` に、`primary`（300分）と`secondary`（10080分）の `used_percent`、`window_minutes`、`resets_at` が記録される。最新スナップショットから正確に表示できる。
- Claude Code: 通常のJSONLと `stats-cache.json` にはトークン使用量はあるが、通常時の5時間・週次使用率は安定して保存されていない。`quotaLimits` は上限到達時などに限って現れる。トークン数から契約上限比を正確に逆算することもできない。
- Claude Codeの正確な使用率は、ローカルでClaude CLIをPTY起動し、`/usage` を送ってTUI出力を解析する方式を検討する。Webや非公開APIへ直接アクセスせずローカル完結できる一方、TUI変更への追従が必要。
- 想定構成: CodexはJSONLを直接読む。ClaudeはPTY経由の`/usage`取得を使う。取得結果はローカルDBへ時系列保存し、同じ画面で表示する。

### トークン使用量の時間推移（表示要件確定）

両CLIともローカルJSONLから取得可能。

- Codex CLI: 各 `token_count` イベントの `timestamp` と `last_token_usage` を使用する。`total_token_usage` はセッション内累積なので、そのまま全イベント分を合計しない。
- Claude Code: assistantメッセージの `timestamp` と `message.usage` を使用する。同じAPI応答が複数のJSONL行へ記録される場合があるため、`message.id`、`requestId`、usage内容などで重複排除してから集計する。
- 時間推移グラフは5時間単位のトークン使用量だけを表示する。使用上限率自体の履歴グラフは作らない。
- input/output/cache/reasoning等を系列別に集計する。
- 単一デバイスのローカルログだけを対象とし、別デバイスで発生した利用との差は許容する。複数端末の同期・集約はスコープ外とする。
- 現在の5時間枠・週次枠の使用率は円形ゲージで表示する。Codexは最新JSONLの `rate_limits`、ClaudeはPTY経由で実行した `/usage` の解析結果を使う。

## Tauriローカルアプリ初期実装（2026-08-30）

- React + TypeScript + Tauri 2で初期実装した。
- Codex/Claudeの5時間枠・週次枠を4枚の円形ゲージで表示する。
- トークン推移は現在時刻から遡る連続5時間区間へ集計し、Input、Output、Cache read、Cache write、Reasoningを積み上げ棒グラフで表示する。
- CodexはJSONLの `last_token_usage` を使用し、再開ログ等に同じイベントが複製された場合の重複排除を行う。
- Claudeは `message.id` または `requestId` で重複排除する。
- Claudeの上限率取得は `portable-pty` でClaude CLIを起動し、`/usage` を送信してANSI除去後の表示を解析する。
- TypeScriptビルド、Rust単体テスト、Clippy、Tauriリリースビルド、Windowsでの短時間起動を確認済み。
