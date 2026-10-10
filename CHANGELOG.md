# 変更履歴

このファイルの形式は [Keep a Changelog](https://keepachangelog.com/ja/1.1.0/) に、版のつけ方は
[セマンティックバージョニング](https://semver.org/lang/ja/) に従う。版は `Cargo.toml` の
`version` が正。手順は [CLAUDE.md](CLAUDE.md) の「版・変更ログ・push」を参照。

## [0.1.1] - 2026-10-10

### 修正

- macOS の CI で、列の境目をキーで動かすテストが落ちていた。テストが `Alt+Shift+矢印` を送っていたため（macOS の既定は `Cmd+Ctrl+矢印` で、アプリの動きは正しい）。
- macOS のビルドで、コアの構成を読む部品が使われないという警告が出ていた（macOS は構成を読むまで 1 論理 CPU を 1 コアとして描く）。

### 変更

- `scripts/verify.sh` と CI の lint に macOS（`aarch64-apple-darwin`）の clippy を足した。Linux からの型チェックだけで、macOS だけで使われないコードを見逃さない。

## [0.1.0] - 2026-10-10

### 追加

- 最初の版。egui で作ったキーボード操作のシステムモニター。
- 画面は overview（`1`）・cpu（`2`）・mem（`3`）・procs（`5`）の 4 枚。残りの 6 枚（gpu·npu・svc・net・disk・sensors・sys）はタブだけ置き、番号を固定した。
- cpu の画面は物理コアごとのタイルを L3 の塊（Ryzen の CCD、Intel の P / E コア）ごとに並べ、タイルの中に SMT のスレッドの棒を描く。
  構成は Linux では sysfs、Windows では `GetLogicalProcessorInformationEx` から取る。macOS は 1 論理 CPU を 1 コアとして描く。
- procs の画面: 絞り込み（`/`）、並べ替え（`s`）、選んだプロセスの詳細と CPU・メモリのグラフ。
- 3 列の幅をマウスのドラッグと `Alt+Shift+矢印`（macOS は `Cmd+Ctrl+矢印`）で変えられる。画面ごとに `state.toml` に残る。等分に戻すのは `Ctrl+Shift+e`。
- サンプリングの間隔（`+` / `-`、0.5〜10 秒）と一時停止（`P`）。何もしていないときは再描画しない。
- 設定は uchmk のアプリで共通のフォルダ（`<config dir>/uchmk/`）に置く。`common.toml` の `language`、`mimamori/config.toml`、yazi 形式の `mimamori/keymap.toml`。
- 画面の文字はすべて言語の表（`src/lang/en.toml`）を通す。この版の言語は英語だけ。
- `--keys` で台本どおりにキーとマウスを送り、スクリーンショットと状態を書き出す（`scripts/xrun.sh`）。
