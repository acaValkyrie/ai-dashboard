# AI Usage

Claude CodeとCodex CLIのローカル利用状況を表示するTauriデスクトップアプリです。

- 現在の5時間枠・週次枠の上限使用率を円形ゲージで表示
- ローカルJSONLから直近5時間のトークン使用量を10分単位で積み上げ表示
- Codexの上限率はJSONL、Claudeの上限率はClaude CLIの`/usage`から取得
- 利用データを外部サービスへ送信しない

## 開発

Node.js、Rust、Claude Code CLIが必要です。

```powershell
npm install
npm run tauri dev
```

リリース実行ファイルを作る場合:

```powershell
npm run tauri build
```

## データ取得

- Codex: `~/.codex/sessions/**/*.jsonl`
- Claude: `~/.claude/projects/**/*.jsonl`
- Claudeの現在の上限率: Claude CLIをローカルPTYで起動して`/usage`を解析

トークン数はモデルやサービス間で同一尺度ではありません。グラフは各ツール内の推移を見るためのもので、契約上限との比較には上部の円形ゲージを使用します。
