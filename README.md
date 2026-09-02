# AI Usage

Claude CodeとCodex CLIのローカル利用状況を表示するTauriデスクトップアプリです。

- 現在の5時間枠・週次枠の上限使用率を円形ゲージで表示
- ローカルJSONLから直近5時間のトークン使用量を10分単位で積み上げ表示
- Codexの上限率はJSONL、Claudeの上限率はClaude Codeと同じOAuth使用率APIから取得
- 起動時・10分ごと・更新ボタン操作時にデータを更新
- 利用データを外部サービスへ送信しない

## 開発

Node.jsとRustが必要です。

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
- Claudeの現在の上限率: Claude Codeと同じ内部OAuth使用率API（非公開仕様）から取得

トークン数はモデルやサービス間で同一尺度ではありません。グラフは各ツール内の推移を見るためのもので、契約上限との比較には上部の円形ゲージを使用します。

### 端末をまたぐかどうか

上の円形ゲージは端末をまたぎます。下の時間ごとの棒グラフはこのアプリを実行している端末の使用量だけです。

- 円形ゲージ: Claudeはアカウント単位のサーバー側APIから取得するため、どの端末で見ても同じ値になります。Codexはローカルのセッションログに記録された値を読むだけなので、その端末で直近にCodexを使っていないと値が更新されず、古い状態のまま表示されることがあります。
- 棒グラフ: Claude・Codexともにその端末のローカルJSONLログのみを集計しているため、他の端末での利用は一切反映されません。
