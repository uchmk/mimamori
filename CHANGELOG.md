# 変更履歴

このファイルの形式は [Keep a Changelog](https://keepachangelog.com/ja/1.1.0/) に、版のつけ方は
[セマンティックバージョニング](https://semver.org/lang/ja/) に従う。版は `Cargo.toml` の
`version` が正。手順は [CLAUDE.md](CLAUDE.md) の「版・変更ログ・push」を参照。

## [0.12.0] - 2026-10-10

### 追加

- disk の画面: 右の列の先頭に、選んだドライブの SMART の健康状態（`Health`）と温度（`Temperature`）を出す。
  健康なら緑、故障が近いと出たら赤で「failing: back it up」とし、左の一覧でもそのドライブの名前を赤くする。
  読めないときは理由を淡く出す（checking…・no SMART data・needs smartctl・needs root・needs administrator・spun down, left asleep）。
- 健康状態は 1 分に 1 回（ドライブの顔ぶれが変わったときはすぐ）、専用のスレッドで聞く。
  - Windows: `IOCTL_STORAGE_PREDICT_FAILURE`。先に `GetDevicePowerState` を見て、止まっている HDD は起こさない。権限が足りなければ「needs administrator」。
  - Linux: `smartctl -H -A -n standby`（smartmontools）の出力を読む。止まっているドライブは起こさず、無いとき・root でないとき・SMART の無い装置（virtio など）は理由を出す。
  - macOS: `diskutil info -plist` の `SMARTStatus`。
- 温度は警告の値と一緒に出し（「41 °C (warns at 70 °C)」）、警告の値を超えたら黄、危険の値を超えたら赤にする。
  - Windows: `StorageDeviceTemperatureProperty`。SSD は 5 秒ごと、ほかのドライブは健康状態の確認と一緒に 1 分ごと。
  - Linux: ドライブの hwmon（NVMe・`drivetemp`）、無ければ `smartctl` の読み値。macOS はまだ温度を読まない。
- sensors の画面: ドライブの温度を足した。hwmon のチップとしてすでに出ているものは重ねない。
- 新しいクレートは足していない（`windows` クレートに `Win32_System_Power` の機能を足しただけ）。

## [0.11.0] - 2026-10-10

### 追加

- disk の画面: macOS でもドライブを読むようにした。IOKit の `IOBlockStorageDriver` ごとに、`Statistics` の読み書きのバイト数（起動からの合計と毎秒）、
  上の装置の型番・SSD か HDD か・バス（Apple Fabric・PCIe・USB・Thunderbolt など）、下のディスク全体の BSD 名（`disk0`）と大きさを出す。
  マウントしたディスクイメージと空のカードリーダーは出さない。macOS は忙しさを数えないので、忙しさのグラフは出ない。
- ボリュームは `statfs` でマウント元の装置を取り、IOKit を親へたどってドライブに結ぶ（APFS はコンテナを通って物理のパーティションへ）。
  結び先は装置ごとに 1 回だけ聞き、ドライブの顔ぶれが変わったら聞き直す。
- 新しいクレートは足していない（IOKit と CoreFoundation のバインディングは sysinfo がすでに使っている `objc2-io-kit` / `objc2-core-foundation`）。

### 変更

- ドライブを読まない OS の注記を「この OS ではドライブを読まない」に改めた。

## [0.10.0] - 2026-10-10

### 追加

- sys の画面（`0`）を作り、10 の画面がそろった。左の列は変わらない情報: システムのメーカーと型番、OS・カーネル・CPU・起動からの時間と起動した時刻、
  ファームウェア（メーカー・版・日付・UEFI かレガシーか・仮想マシンのものか）、マザーボード、メモリ（載っている量・使っているスロット・載せられる最大・エラー訂正）。
  空の値は行ごと出さない。真ん中の列は空きも含むメモリのスロット（場所・容量・種類・速さ・メーカー。空きは淡く）、右の列は選んだスロットの
  バンク・形・定格と実際の速さ・ランク・電圧・型番。パンくずは「機械 · 32.0 GiB in 2 of 4 slots · 読んだ場所」。
- Windows と Linux はファームウェアの SMBIOS の表を読む。Windows は `GetSystemFirmwareTable('RSMB')` で管理者が要らず、起動の方式は `GetFirmwareType`。
  Linux は `/sys/firmware/dmi/tables/DMI` を root のときに読み、ふだんは `/sys/class/dmi/id`（スロットは出ず、root で動かせば出ると言う）、
  SMBIOS の無い ARM の板はデバイスツリーの `model`。macOS は `system_profiler -xml`（Intel の Mac はスロット、Apple シリコンはチップのパッケージの中の 1 塊）。
  起動時に 1 回、専用のスレッドで読むので、最初の CPU の読み値を待たせない。シリアル番号は出さない。
- 自作の PC のようにシステムの名前が空のときは、マザーボードの名前で機械を呼ぶ。
- `Tab` で画面を一周するテストが sys も通るようにした。

## [0.9.0] - 2026-10-10

### 追加

- sensors の画面（`9`）を作った。左の列はセンサーを持つチップ（CPU・ドライブ・マザーボードの監視チップ・GPU）と、それぞれの一番熱い温度の 1 分のグラフ、全体の数と一番熱い温度。
  真ん中の列は温度とファンの一覧（チップ・センサー・今・1 分の最低と最高・高温の目安・限界。温度は目安の 10 °C 手前で黄、超えると赤、ファンは下限を割ると赤）。
  右の列は選んだセンサーのグラフ（目安と限界の線つき）と、読んだ場所。チップで `Enter` を押すとその最初のセンサーへ、センサーで押すとそのチップへ移る。
- Linux は `/sys/class/hwmon`（温度とファン、目安と限界。ドライブは型番つき）を読み、どのチップにも温度が無ければ `/sys/class/thermal` の熱ゾーンを出す。
  Windows は `Thermal Zone Information` のパフォーマンスカウンター（PDH）で ACPI の熱ゾーンを管理者なしで読む（ファンとコアの温度はメーカーのドライバーが要るので、そう書く）。
  macOS は sysinfo の components（SMC と HID の温度）。NVIDIA の GPU は NVML の温度とファンを足し、amdgpu のように OS が並べているものは重ねない。
- センサーは画面を開いていなくても毎回読むので、開いたときから 1 分のグラフがある。

## [0.8.0] - 2026-10-10

### 追加

- disk の画面（`8`）を作った。左の列はドライブごとの 1 分の読み書きと、全体の読み書き・ボリュームの数・容量・空き。
  真ん中の列はマウントされたボリューム（マウント先・デバイス・ファイルシステム・容量・使用・空き・使用率・読み書きの速さ。使用率は 75 % で黄、90 % で赤）。
  右の列は選んだドライブの読み・書き・ビジーのグラフと、型番・種類（SSD / HDD）・バス・容量・起動からの合計・載っているボリューム、選んだボリュームの詳しい値。
  ドライブで `Enter` を押すとその最初のボリュームへ、ボリュームで押すとそのドライブへ移る。
- Windows は PhysicalDisk のパフォーマンスカウンター（PDH）で読み書きとビジーを、`\\.\PhysicalDriveN` への `IOCTL_STORAGE_QUERY_PROPERTY` で型番・バス・SSD / HDD を
  （管理者なしで、ドライブごとに 1 回）読む。カウンターが無効なら左の列でそう言う。Linux は `/proc/diskstats` と `/sys/block`。macOS はまだボリュームだけ。
- ドライブとボリュームは画面を開いていなくても毎回読むので、開いたときから 1 分のグラフがある。

## [0.7.5] - 2026-10-10

### 追加

- net の画面で、左の列のインターフェースで `Enter` を押すと、真ん中のソケットをそのインターフェースのもの（ローカルのアドレスがそのインターフェースのアドレス）に絞るようにした。
  どのアドレスにも付いたもの（`0.0.0.0`・`::`）はそのインターフェースの通信も受けるので残す。真ん中の列の見出しにインターフェースの名前を出し、選択を動かせば絞り込みも追う。

### 変更

- net の画面のインターフェースで `Enter` は、真ん中の列へ移るだけだったのを、移る前にソケットを絞るようにした。
- net の画面の `Esc` は、文字の絞り込みに加えてインターフェースの絞り込みも外す。

## [0.7.4] - 2026-10-10

### 追加

- macOS の net の画面で、ソケットの一覧を読むようにした（`netstat -anvW` のパース。TCP と UDP、IPv4 と IPv6、状態と PID）。
  PID の無い行が root なしで出るときだけ、左の列に「他の利用者のソケット」と出す。
- macOS のインターフェースの種類を、名前から推すのをやめ、`networksetup -listallhardwareports` のハードウェアポートで決めるようにした
  （Wi-Fi・Ethernet・Bluetooth PAN・Thunderbolt Bridge）。右の列の `Adapter` にポート名を出す。ポートの無いもの（`utun`・`awdl` など）は今までどおり名前から推す。

## [0.7.3] - 2026-10-10

### 追加

- Windows の svc の画面で、ドライバー（カーネルとファイルシステムのドライバー、`SERVICE_DRIVER`）も読むようにした。
  サービスと混ぜず、`u` でサービス ⇄ ドライバーを切り替える（procs の自分だけ・net の待ち受けだけと同じ鍵）。左の列に今の種類を出す。
- ドライバーの開始・停止の確かめる窓には、同じことをするコマンドとして `sc.exe start|stop <名前>` を出す。macOS と Linux で `u` を押すと、ドライバーは Windows だけとトーストで言う。

## [0.7.2] - 2026-10-10

### 修正

- macOS の svc の画面で、読み込まれていないジョブ（`stopped (not loaded)` と出る）の開始・再起動が `launchctl kickstart` の「見つからない」で失敗していた。
  有効なジョブは plist を `launchctl bootstrap <ドメイン> <plist>` で読み込んでから `kickstart` するようにした。確かめる窓にも 2 つのコマンドを並べて出す。
- 無効のジョブは、無効を解くと持ち主の設定を変えてしまうので触らない。`S` / `R` を押すと、元に戻すコマンド（`launchctl enable gui/<uid>/<label>`）をトーストで言うだけにした。

## [0.7.1] - 2026-10-10

### 追加

- macOS の svc の画面で、launchd の plist も読むようにした。利用者なら `~/Library/LaunchAgents`・`/Library/LaunchAgents`・`/System/Library/LaunchAgents`、
  root なら `/Library/LaunchDaemons`・`/System/Library/LaunchDaemons`（`svcctl` が操作するのと同じドメイン）。
  - 読み込まれていないジョブ（無効のものなど）も、止まっているジョブとして一覧に出る。無効かどうかは `launchctl print-disabled` を plist の `Disabled` より優先する。
  - どのジョブにもコマンド（`ProgramArguments` / `Program`）・ファイル・アカウント（`UserName`。デーモンで無ければ root）・起動の種類
    （`keep alive`・`at load`・`keep alive if`・`on a schedule`・`on demand`・`disabled`）が出る。
  - plist は XML もバイナリも読む（Pure Rust の `plist` クレート）。読んだものはファイルの時刻が変わるまで覚えておく。

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
