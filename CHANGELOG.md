# 変更履歴

このファイルの形式は [Keep a Changelog](https://keepachangelog.com/ja/1.1.0/) に、版のつけ方は
[セマンティックバージョニング](https://semver.org/lang/ja/) に従う。版は `Cargo.toml` の
`version` が正。手順は [CLAUDE.md](CLAUDE.md) の「版・変更ログ・push」を参照。

## [0.7.0] - 2026-10-10

### 追加

- svc の画面（`6`）で、選んだサービスを `S` で開始、`x` で停止、`R` で再起動できるようにした。どれも先に確かめる窓が出て、
  サービスの名前・説明・状態・起動の種類・PID と、同じことをするコマンド（`systemctl restart foo.service`・`launchctl kickstart -k gui/501/…`・
  `Restart-Service foo`）を見せる。`y` / `Enter` で実行、`n` / `Esc` / `q` でそのまま。実行は 1 回ごとのスレッドで行い、終わったら一覧をすぐ読み直す。
- Windows はサービス コントロール マネージャー（`StartServiceW`・`ControlService`、再起動は止まるのを 30 秒まで待ってから開始）、
  Linux は `systemctl`（デスクトップなら polkit のパスワードの窓が出る）、macOS は一覧を読んだのと同じドメイン（root なら `system/`、ほかは `gui/<uid>/`）に
  `launchctl kickstart` / `kill SIGTERM` を送る。
- 権限が足りないときは OS ごとの言い方でトーストに出す（Windows は管理者として起動する、Linux は root か polkit、macOS はシステムのジョブに `sudo`）。
  動いているサービスの開始・止まっているサービスの停止は、確かめずにトーストで言うだけ。
- コマンド `service start` / `service stop` / `service restart` を足した。

### 変更

- **既定キー:** `x`（`kill`）は svc の画面ではサービスの停止になる。ほかの画面では今までどおりプロセスの終了。
- 確かめる窓（`[confirm]`）のキーの説明を「Yes」/「No, leave it as it is」にした（プロセスとサービスのどちらにも使うため）。

## [0.6.1] - 2026-10-10

### 修正

- net の画面で、Bluetooth PAN（スマートフォンのテザリング）のインターフェースの種類が「wired」と出ていたのを「Bluetooth」にした。
  Windows は `MIB_IF_ROW2` の `PhysicalMediumType`（Bluetooth は ifType が Ethernet なので、こちらでしか分からない）、
  Linux は `/sys/class/net/*/uevent` の `DEVTYPE=bluetooth`（無い古いカーネルでは `bnep` で始まる名前）で見分ける。
  Bluetooth のアダプターそのものはネットワークのインターフェースではないので、一覧には出ない。macOS は種類を名前から推すまま。

## [0.6.0] - 2026-10-10

### 追加

- net の画面（`7`）を足した。左の列はネットワークのインターフェース（それぞれの今の送受信の速さと直近 1 分の小さなグラフ、切れているものは「down」）と、
  ソケットの数（待ち受け・確立・すべて）、一覧が短いときの理由。中の列は TCP と UDP のソケットの一覧（プロトコル・ローカル・リモート・状態・PID・プロセス）。
  右の列は選んだインターフェースの受信と送信のグラフ、アダプター・種類・リンクの速さ・IPv4 / IPv6・MAC・MTU・起動からの合計と、選んだソケットの詳しい情報。
  ソケットで `Enter` を押すと procs を開き、そのプロセスを選ぶ。
- net では `u` で待ち受けのソケット（TCP の `LISTEN` と、相手の無い UDP）だけにする。`/` はプロトコル・アドレス・状態・PID・プロセス名で絞り込む（procs・svc と共通の絞り込み）。
- Windows は IP ヘルパー（`GetIfTable2`・`GetUnicastIpAddressTable`、ソケットは `GetExtendedTcpTable` / `GetExtendedUdpTable`）を読む。
  物理アダプターは抜けていても出し、ほかは繋がっていてアドレスのあるもの（VPN・Hyper-V のスイッチ）だけ。フィルターの層は出さない。
- Linux はインターフェースを sysinfo と `/sys/class/net`（種類・速度・ドライバー）から、ソケットを `/proc/net/{tcp,tcp6,udp,udp6}` から読み、
  持ち主のプロセスは `/proc/*/fd` の `socket:[inode]` から探す。root でないとき、ほかの利用者のソケットにはプロセスが出ない（左の列にそう出す）。
- macOS はインターフェースだけ（種類は名前から推す）。ソケットの一覧はまだ。
- インターフェースは間隔ごとに読み（画面を開いたときに 1 分のグラフがある）、ソケットは net の画面が出ている間だけ 2 秒ごとに読む。

## [0.5.0] - 2026-10-10

### 追加

- svc の画面（`6`）を足した。読むだけ。左の列は表示（すべて / 動作中 / 停止 / 失敗、それぞれの数）と、開始の種類（自動 / 手動 / 無効）ごとの数、
  一覧が短いときの理由。中の列は状態・名前・開始・PID の一覧（`/` で名前と説明を絞り込む。絞り込みは procs と共通）。
  右の列は選んだサービスの説明・状態・開始・PID・最後の終了コード・実行するアカウント・コマンド・ファイル。`Enter` で procs を開き、そのプロセスを選ぶ。
- Windows はサービス コントロール マネージャー（`EnumServicesStatusExW`、開始の種類・アカウント・コマンドは `QueryServiceConfigW` で 1 分キャッシュ）を読む。ドライバーは出さない。
- Linux は `systemctl list-unit-files` と `systemctl show '*.service'` を読む。systemd が動いていない（コンテナ、systemd の無い WSL）ときはユニットファイルだけを、状態を「unknown」として出す。
- macOS は `launchctl list` を読む（利用者として起動したときは、その利用者のジョブだけと出す）。
- サービスは svc の画面が出ている間だけ 5 秒ごとに読む（入ったときはすぐ、一時停止中でも 1 回読む）。

## [0.4.0] - 2026-10-10

### 追加

- gpu·npu の画面（`4`）を足した。左の列は見つかった GPU と NPU（それぞれの直近 1 分の使用率）と、無いときはその理由。
  中の列は選んだ装置の使用率のグラフ・エンジンごとの使用率・専用 / 共有メモリ・VRAM のグラフ。右の列は温度・ファン・電力・クロック、
  読めない値とその理由、その装置を使っているプロセス（`Enter` で procs を開き、そのプロセスを選ぶ）。
- Windows は PDH の `GPU Engine` / `GPU Adapter Memory` / `GPU Process Memory` のカウンタを読み、DXGI で名前を付ける。MCDM のドライバーの NPU も同じカウンタから出す。
- Linux は `/sys/class/drm`（amdgpu の使用率・VRAM・GTT・hwmon のセンサー、i915 / xe のクロック）と `/sys/class/accel`（`intel_vpu` の使用率とクロック、`amdxdna` は一覧だけ）を読む。
- NVIDIA は NVML をドライバーから実行時に読む（ビルドには何も要らない）。Linux では NVIDIA のカードの出どころ、Windows ではセンサーとプロセスのメモリを足す。無ければ Linux では sysfs から名前だけを出し、Windows ではセンサーが「—」になる（どちらも理由を出す）。
- macOS は今は何も読まず、理由を出す（GPU の IOKit は未実装、Neural Engine はほかのプログラムに使用率を出さない）。

## [0.3.0] - 2026-10-10

### 追加

- procs を親子の木で表示できるようにした（`t` で木と一覧を切り替える）。名前の前に `├ │ └` の枝を引き、兄弟は今の並べ替えの順に並ぶ。
  絞り込みやスコープで親が隠れているプロセスは、見えている一番近い祖先の下に付く（無ければ根になる）。
  子より後に起動した「親」は、同じ PID を使い回した別のプロセスとして扱い、その下には付けない。左の列に「View」（Tree / List）を出す。
- スコープ（All / Mine）を切り替えるキーを足した（`u`）。左の列の選択と同じもので、トーストに今のスコープが出る。
- コマンド `tree` と `scope`（`scope all` / `scope mine` で直接選ぶ）を足した。

## [0.2.0] - 2026-10-10

### 追加

- procs でプロセスを終了できるようにした（`x`）。終了の前に、名前・PID・ユーザー・コマンドと終わらせ方を出す確認のオーバーレイを挟み、`y` / `Enter` で終了、`n` / `Esc` / `q` でやめる。
  Windows は `TerminateProcess`、macOS と Linux は SIGTERM を送り、3 秒たっても残っていれば SIGKILL。結果（終了した・SIGKILL した・もう無かった・OS に断られた理由）はトーストで出る。
  overview・cpu・mem の上位のプロセスの一覧でも使える。
- 終了は UI のスレッドの外で行い、送る直前に PID と起動時刻を照らし合わせる（確認の間にプロセスが終わり、同じ PID が別のプロセスに使われても巻き込まない）。自分自身（mimamori）は終了しない。
- keymap に `[confirm]` の節と `kill` のコマンドを足した。

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
