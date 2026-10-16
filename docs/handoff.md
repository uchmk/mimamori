# 引き継ぎ（2026-10-16）

持ち主がセッションを切り替えるために書いた。6 つのリポジトリ（yagura・kura・tsumugi・ito・minato・michi）にまたがる作業の、yagura から見た状態。
**返答は日本語**（コード・コメント・コミットは英語）。

## yagura の状態

- ローカルの `main` は c6e8627（v0.65.1）で、`origin/main` より 4 コミット遅れている（先頭は 723e65b、v0.67.0）。作業の前に `git merge --ff-only origin/main`。
  - 69a8512（版なし）: レーンの回が起こすエージェントは 127.0.0.1 で待ち受ける（ファイアウォールの窓を出さない。tsumugi #24）。
  - 723e65b v0.67.0: webhook を幾つでも、`[[notify.webhooks]]` の表ごとに（ito 0.47.0。tsumugi #164）。再テスト yagura #61。
- 開発のセッションが進められる TODO は `scripts/todo-open.sh` で 4 件。印の付いたもの（【人】【実機】など）は進めない。

## ほかのリポジトリ（22:55 時点）

| リポジトリ | main | 版 |
| --- | --- | --- |
| yagura | 723e65b | v0.67.0 |
| kura | 6c85efe | v0.117.0（ito は 0.46.1。0.47.0 は kura-5b に頼んだ） |
| tsumugi | bdc0c2b | v0.115.0 |
| ito | 37bf3de | v0.47.0 |
| minato | 1dca63d | v0.52.0（ito は 0.42.0。0.47.0 は minato-42 に頼んだ） |
| michi | 4476bf7 | v0.32.0 |

## 開いているもの

- **minato #63**（Release v0.50.0、`needs-pc`）: `gh workflow run release.yml -R uchmk/minato --ref main -f tag=v0.50.0` が自動モードの確認に止められた。
  持ち主が `!` で回すか、許可の規則を足すのを待つ。止められたことを回避しない。ほかのセッションに頼まない。答え待ちは 2 回聞いたので、もう聞かない。
- **#164 の後始末**: kura・minato が ito 0.47.0 を取り込んだら、#164 に版をコメントで足す（閉じたままでよい）。
- kura #393（finding）: `verify.sh` の途中でテストが一度だけ 60 秒以上止まった。再現しない。何もしない。
- question はほかのセッションのもの: kura #418、minato #64、michi #10〜#22。
- bug は全リポジトリで 0 件。

## 守ること

- 直す前に、ほかのセッションが同じ issue に手を付けていないかを見る（`git fetch`、issue の timeline、リモートのブランチ、ListAgents）。
- `C:\dev\{yagura,kura,ito,minato,michi,tsumugi}` のクローンと `*-wintest`・`*-armtest` のレーンはほかのセッションのもの。
  tsumugi にはコミットしていない変更がある。そこでは `fetch` と `worktree add/remove` だけ。
- 作業は `origin/main` から `C:\dev\<repo>-verify` に worktree を作り、中で `cargo-ramdisk.ps1` を回してからビルドする（ソースは R: に置かない）。
  片付けは `cargo clean` → `target` のジャンクションを `[System.IO.Directory]::Delete` → `git worktree remove --force` と `prune` → `rm -rf /r/cargo-target/<repo>-verify`。
- push の前は `scripts/verify.sh`（`ALL OK` を確かめる）。`origin/main` が進んでいれば merge コミットで取り込み、`verify.sh` を回し直す。
- ito の公開の API を変えたら CHANGELOG に「使う側がやること」を書く。`rev` を上げたら `scripts/lanes.sh sync`。
- 持ち主の決めたこと: macOS のランナーでテストを回さない（ビルドだけ）。ring を残す。
- やらないこと: 持ち主のアカウントで Claude Code を動かして再現する、持ち主の PC で tsumugi の GUI を動かす。
  持ち主のメールアドレスを外のサービスに送らない。

## 道具の癖

- rtk が出力を縮める。生のまま見るには `rtk proxy …`（`git log` のマージ、`cargo test`、`git diff` など）。
- JST の日付は PowerShell の `[System.TimeZoneInfo]::ConvertTimeBySystemTimeZoneId([DateTime]::UtcNow, 'Tokyo Standard Time')`（Git Bash の `TZ=Asia/Tokyo date` は UTC を返す）。
- Bash の呼び出しの `cd` は主の作業フォルダごと動く。`git -C` か、毎回 `cd` から始める。
- `gh run list --commit` は完全な SHA が要る。PR の CI の re-run は古い main で回るので、`gh pr update-branch <N>`。
