# mimamori — Claude 向けメモ

キーボードで操作するシステムモニター。Rust + egui 0.36、Windows 優先で macOS / Linux（それぞれ x86_64 / ARM64）も動かす。
姉妹プロジェクトは [filer](https://github.com/uchmk/filer)（yazi 風のファイルマネージャー）と [tsumugi](https://github.com/uchmk/tsumugi)（AI CLI のためのターミナル）。
見た目・キー・設定の置き場所は filer と揃える。画面の設計の元は HTML の見本（https://claude.ai/artifact/QhsVSAEHsUZmxbZbqyuGjj の「設計」タブ）。

## 作業ルール

- 返答は日本語。コード・コメント・コミットメッセージは英語。
- 頼まれるまでコミットしない。
- 改行は LF（`.gitattributes` の `eol=lf`）。スクリプトで書き換えるときは改行を変えない（Python なら `newline=''`）。
- タスクは [TODO.md](TODO.md)。実機でしか確かめられないことは [TESTING.md](TESTING.md) に積む（機能を足したら行を足してから完了にする）。
  `[x]` を付けてよいのは実機のセッションと持ち主だけ。動きを変えた行は `[ ]` に戻す。

## 版・変更ログ・push

一人開発なので `main` への直接のコミット・push を許す。マージは merge コミット（rebase / squash を使わない）。`--force` は使わない。

- semver。版は `Cargo.toml` の `version` が正。PATCH = 修正・ドキュメント・小さな整理、MINOR = 機能・大きめの変更。
  既定キーや設定の互換性に関わる変更は CHANGELOG に**変更**として明記する。
- **版の繰り上げと [CHANGELOG.md](CHANGELOG.md) への追記はセット。**`version` を書き換える → `cargo build` で `Cargo.lock` も更新
  （忘れると CI の `--locked` が落ちる）→ CHANGELOG の先頭に節（Keep a Changelog、日本語、**日付は JST**: `TZ=Asia/Tokyo date +%F`）→ コードと一緒にコミット。
- コミットの件名は `vX.Y.Z: …`、最初の段落は英語で単体で通じるように書く（後でリリースノートにする）。
- **push の前は `scripts/verify.sh`**（build `--locked`・`fmt --check`・test・clippy を Linux・`x86_64-pc-windows-msvc`・`aarch64-apple-darwin` で。最後の行が `ALL OK: …` なら push してよい）。

## ビルド・検証

- **filer / tsumugi と違い、`cargo fmt` を使ってよい**（`rustfmt.toml` で `max_width = 140`）。CI は `cargo fmt --check` を見る。
- clippy は `--all-targets -- -D warnings` で警告ゼロ。Linux では `#[cfg(windows)]` のコードがコンパイルされないので、Windows と macOS のターゲットの clippy も回す（macOS だけで使われないコードの警告も、Linux からはここでしか出ない）。
- テストは機械の設定ファイルに左右されてはいけない。一時ディレクトリは `util::test_dir("ラベル")`、設定は `settings::load(Some(dir))` に渡す。
- **画面は Xvfb で確かめる**: `scripts/xrun.sh OUTDIR --keys '<Wait:2000>2<Shot:cpu><State:a>'`。
  - `<Shot:名前>` は PNG、`<State:名前>` は画面・フォーカス・選択・列の幅などの文字を OUTDIR に書く。
  - `<Click:x,y>` と `<Drag:x1,y1-x2,y2>` の座標は窓に対する割合（0〜1）。境目を掴むときは `<State>` の `widths`（1000 分率）から位置を出す。
  - 設定は OUTDIR/config から読む（`UCHMK_CONFIG_DIR`）。前の回の列の幅は持ち越さない。
  - `XRUN_IDLE=10` で、最後の 10 秒の CPU 使用率を `cpu.txt` に書く。Xvfb は llvmpipe（ソフトウェア描画）なので、
    再描画 1 回が重く、1 秒ごとのサンプリングで 1 コアの 10% 前後に見える。mimamori 自身（収集 0.1%・UI 0.7% ほど）とは分けて読む。

## 設計の約束事

- **ディスクやプロセスに触る処理は UI スレッドで実行しない。**収集は `collect/` の 1 本のスレッドで、Source ごとに間隔を持ち、
  `Msg` を crossbeam で UI に送って `request_repaint` で起こす。間隔と一時停止は制御のチャネルで送る。
- **何もしていないときは再描画しない。**`request_repaint_after` はフレームの中で呼ぶ（`raw_input_hook` の中で頼んだものはフレームの頭で消える）。
  トーストのように時間で消えるものは、残り時間で起こす（`App::toast_left`）。
- **キーは keymap 経由**（`src/defaults/keymap.toml`、利用者の `keymap.toml` が上に重なる）。ハードコードしない。
  修飾キーは `<P-…>` が macOS で Cmd、ほかで Ctrl。`on_mac` で macOS だけ別のキーにできる。
- **画面の文字はすべて `tr("key")` / `trf("key", &[..])` を通す。**英語の表は `src/lang/en.toml`。
  言語は `config.toml` → `common.toml`（uchmk のアプリで共通）→ OS の順で決まる。表に無いキーはテストが落とす。
- **列の大きさは `tsumugi-layout`**（tsumugi の workspace のクレート、`Cargo.toml` の `rev` で固定）。直すときは tsumugi 側で直して `rev` を上げる。
- **マルチ OS**: OS ごとの処理は `#[cfg(...)]` のモジュールに閉じ込める（例: `collect/topo/linux.rs` と `windows.rs`、macOS は `fallback`）。
  パースは純粋な関数にして、どの OS でもテストが回るようにする（`#[cfg(any(windows, test))]`）。
- **マルチアーキテクチャ**: Pure Rust のクレートを優先し、C のライブラリを抱えるものは避ける。
- 新しい画面を作るときは `Screen::built` に足し、`ui/` に描画を置き、keymap と README のキー表・画面の表・TESTING.md を更新する。

## 構成

| 場所 | 役割 |
| --- | --- |
| `src/main.rs` | eframe の起動、`--keys` の受け付け、フレームごとの `pump` と描画 |
| `src/app.rs` | アプリの状態と `Act` の実行（画面・フォーカス・選択・絞り込み・並べ替え・列の幅・トースト） |
| `src/collect/` | 収集のスレッド（cpu / mem / proc / gpu / net のインターフェース / disk / sensors、svc と net のソケットは画面が出ている間だけ、hw は起動時に 1 回だけ専用のスレッドで）。`topo/` は起動時に 1 回取るコアの構成（Linux は sysfs、Windows は Win32） |
| `src/collect/gpu/` | GPU・NPU。`linux.rs`（drm と accel の sysfs）、`windows.rs`（PDH のカウンタを装置ごとにまとめる純粋な関数）と `pdh.rs`（PDH・DXGI の呼び出し）、`nvidia.rs`（NVML を実行時に読む）。取れない値は `Why` で理由を持つ |
| `src/collect/svc/` | サービス。`linux.rs`（`systemctl` の出力のパース）、`windows.rs`（SCM の数値を状態に畳み、ドライバーを見分ける純粋な関数）と `scm.rs`（SCM の呼び出し、サービスとドライバー、設定は 1 分キャッシュ）、`mac.rs`（`launchctl list` と `print-disabled`、launchd の plist。plist はファイルの時刻でキャッシュ）。一覧が短い理由は `Note` |
| `src/collect/net/` | ネットワーク。インターフェースは sysinfo の数え値に、`linux.rs`（sysfs の種類・速度・ドライバー）と `windows.rs`（IP ヘルパーの表から人が思うアダプターを選ぶ純粋な関数）と `iphlp.rs`（IP ヘルパーの呼び出し）、`mac.rs`（`networksetup` のハードウェアポートで種類、`netstat -anvW` のパースでソケット）。ソケットは Linux が `/proc/net` と `/proc/*/fd`、Windows が `GetExtendedTcpTable` / `GetExtendedUdpTable` |
| `src/collect/disk/` | ディスク。ボリュームはどの OS も sysinfo。ドライブは `linux.rs`（`/proc/diskstats` と `/sys/block` のパース、ボリュームの持ち主のドライブ）、`windows.rs`（PDH のインスタンス名と `STORAGE_DEVICE_DESCRIPTOR` を読む純粋な関数）と `ioctl.rs`（PDH と `IOCTL_STORAGE_QUERY_PROPERTY` の呼び出し）、`mac.rs`（IOKit の読み値をドライブに畳む純粋な関数）と `iokit.rs`（`IOBlockStorageDriver` の `Statistics` と、ボリュームの装置から親をたどる呼び出し）。`health.rs` は SMART の健康状態を 1 分に 1 回、専用のスレッドで聞く（Windows は `IOCTL_STORAGE_PREDICT_FAILURE` と `GetDevicePowerState`、Linux は `smartctl` の出力のパース、macOS は `diskutil info -plist`）。温度は Windows が `StorageDeviceTemperatureProperty`、Linux が hwmon。SMART の中身（寿命・時間・書き込み・エラー）は Windows の NVMe が `StorageDeviceProtocolSpecificProperty` の health ログ（パースは OS に依らない `mod.rs` の `nvme_log`）、Linux が smartctl の同じ出力。PDH の配列の読み方は gpu と共有の `collect/pdh.rs` |
| `src/collect/sensors/` | 温度とファン。`linux.rs`（`/sys/class/hwmon` のチップと、温度が無いときの `/sys/class/thermal`）、`windows.rs`（熱ゾーンのカウンターを畳む純粋な関数）と `zones.rs`（PDH の呼び出し）、macOS は sysinfo の components。NVIDIA の GPU は gpu の読み値から足し、PCI のアドレスで重ねない。ドライブの温度は disk の読み値から足し、hwmon のチップ名で重ねない |
| `src/collect/hw/` | sys の画面の変わらない情報。`smbios.rs`（SMBIOS の表のパース。システム・ファームウェア・板・メモリの配列とスロット）、`windows.rs`（`GetSystemFirmwareTable('RSMB')` と `GetFirmwareType`）、`linux.rs`（`/sys/firmware/dmi/tables` → `/sys/class/dmi/id` → デバイスツリーの順）、`mac.rs`（`system_profiler -xml` のパース）。読めない理由は `Note` |
| `src/kill.rs` | プロセスの終了（確認の後、1 回ごとのスレッドで。Windows は `TerminateProcess`、ほかは SIGTERM → SIGKILL） |
| `src/svcctl.rs` | サービスの開始・停止・再起動（確認の後、1 回ごとのスレッドで。Windows は SCM、Linux は `systemctl`、macOS は `launchctl`。権限不足とエラーの読み替えは純粋な関数） |
| `src/tree.rs` | procs の木の並び（親子と枝の線。親が隠れていれば一番近い見えている祖先へ、PID の使い回しは親にしない） |
| `src/model.rs` | 履歴のリングバッファ（60 秒）と最新の値 |
| `src/config/` | `keys.rs`（キーの文字列）、`keymap.rs`、`cmd.rs`（コマンド文字列 → `Act`）、`settings.rs`、`theme.rs`（filer の色） |
| `src/i18n.rs`, `src/lang/` | `tr()` と言語の表 |
| `src/persist.rs` | `state.toml`（列の幅）の読み書き |
| `src/keyscript.rs` | `--keys` の台本の実行（キー・クリック・ドラッグ・スクリーンショット・状態の書き出し） |
| `src/ui/` | 描画。`overview.rs`・`cpu.rs`・`mem.rs`・`procs.rs`・`gpu.rs`・`svc.rs`・`net.rs`・`disk.rs`・`sensors.rs`・`sys.rs`・`tiles.rs`（コアのタイル）・`graph.rs`・`help.rs`・`confirm.rs`（プロセスの終了とサービスの操作の確認） |
| `scripts/` | `verify.sh`（push の前）、`xrun.sh`（Xvfb で起動して撮る） |
