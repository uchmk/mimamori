# TODO

v0.1.0 で overview・cpu・mem・procs の 4 枚ができた。残りは下の順で進める。
`【人】` は持ち主の作業、`【実機】` は実機でしか確かめられないもの（[TESTING.md](TESTING.md) に積む）。

## 1. 残りの画面（番号は v0.1 で固定済み）

- [x] procs: プロセスの終了（`x`、確認のオーバーレイを挟む。Windows は `TerminateProcess`、ほかは SIGTERM → SIGKILL）。
- [x] procs: 木の表示（親子）と、スコープ（All / Mine）の切り替えのキー。
- [x] gpu·npu（`4`）: 使用率・VRAM・温度。Windows は PDH の `GPU Engine` カウンタ、Linux は `/sys/class/drm` と amdgpu / i915 の sysfs、NVIDIA は NVML を動的に読む（無ければ出さない）。NPU は見本の「設計」タブの限界のとおり、取れる OS だけ。
- [x] svc（`6`）: Windows のサービス、Linux の systemd のユニット、macOS の launchd。読むだけから始める。
- [x] svc: 開始・停止・再起動のキー（確認を挟む。Windows は `ControlService`、Linux は `systemctl`、macOS は `launchctl kickstart` / `kill`）。権限が足りないときの言い方も。
- [x] svc: macOS の launchd の plist（`/Library/LaunchDaemons` など）を読み、止まっている・無効のジョブ、コマンドとファイルも出す。今は `launchctl list` だけ。
- [x] svc: macOS で読み込まれていないジョブ（無効のものなど）の開始。今は `launchctl kickstart` が「見つからない」で失敗してトーストに出る。`launchctl enable` と `bootstrap` まで行うか（無効を解くのは持ち主の設定を変えることになる）決める。
  → 有効なジョブは `launchctl bootstrap <ドメイン> <plist>` で読み込んでから `kickstart` する。無効のジョブは無効を解かず、`launchctl enable …` をトーストで教えるだけ（v0.7.2）。
- [x] svc: Windows のドライバー（`SERVICE_DRIVER`）を出すか、左の列の切り替えにするか決める。
  → 一緒に読み、`u`（procs の Mine や net の LISTEN と同じ鍵）でサービス ⇄ ドライバーを切り替える。左の列に今の種類を出す。ドライバーの操作の行は `sc.exe start|stop`（v0.7.3）。
- [ ] svc の実機の確認 `【実機】`（TESTING.md 3b）。
- [x] net（`7`）: インターフェースごとの送受信のグラフ、接続の一覧。Windows は IP ヘルパー、Linux は sysfs と `/proc/net`・`/proc/*/fd`。
- [x] net: macOS のソケットの一覧（`proc_pidinfo` の `PROC_PIDFDSOCKETINFO`、または `netstat -anv` のパース）と、インターフェースの種類（`SCNetworkInterface`。Bluetooth PAN も見分ける）。今はインターフェースだけ。
  → ソケットは `netstat -anvW` のパース（PID の列は見出しから数える。古い `pid` と Sonoma 以降の `process:pid` の両方）。`proc_pidinfo` は他の利用者のプロセスを root なしで覗けず、
  `lsof` は 2 秒ごとには重いので使わない。種類は `networksetup -listallhardwareports` のハードウェアポート名（Wi-Fi・Ethernet・Bluetooth PAN・Thunderbolt Bridge）で、
  1 分ごとに読み直す。SCNetworkInterface のバインディングを抱えずに済む。ポートの無いもの（utun・awdl など）は名前から推す（v0.7.4）。
- [x] net: 選んだインターフェースで接続を絞る（ローカルのアドレスがそのインターフェースのもの）か決める。今は全部を出している。
  → 選ぶだけでは絞らない（左の選択はいつもあるので、ほとんどが隠れる）。インターフェースで `Enter` を押すと絞り、どのアドレスにも付いたもの（`0.0.0.0`・`::`）は残す。選択を動かせば追い、`Esc` で外す（v0.7.5）。
- [ ] net の実機の確認 `【実機】`（TESTING.md 3c）。
- [x] disk（`8`）: ドライブごとの使用量と読み書きの速さ。
  → 左がドライブ（1 分の読み書き）、真ん中がマウントされたボリューム（容量・使用率・読み書き）、右が選んだドライブのグラフ（読み・書き・ビジー）と型番・種類・バス・容量。
  Windows は PhysicalDisk のパフォーマンスカウンター（PDH）と `IOCTL_STORAGE_QUERY_PROPERTY`（管理者は要らない）、Linux は `/proc/diskstats` と `/sys/block`、ボリュームはどの OS も sysinfo（v0.8.0）。
- [x] disk: macOS のドライブ（IOKit の `IOBlockStorageDriver` の `Statistics`）。型番・SSD / HDD・バス・BSD 名と大きさ、ボリュームは `statfs` の装置から IOKit を上って結ぶ。忙しさは macOS に無いので出さない（v0.11.0）。
- [x] disk: SMART の状態と温度（Windows は `MSFT_PhysicalDisk` / `StorageReliabilityCounter`、Linux は NVMe の `hwmon` と `smartctl`）。sensors の画面と合わせて決める。
  Linux の NVMe の温度は v0.9.0 から sensors の画面に出る（hwmon の `nvme`）。残るのは SATA の温度（`drivetemp` が無いとき）と、Windows・macOS の温度、SMART の健康状態。
  → 健康状態は 1 分に 1 回、専用のスレッドで聞く（ドライブの顔ぶれが変わったときも）。Windows は WMI ではなく `IOCTL_STORAGE_PREDICT_FAILURE`（`GetDevicePowerState` で回っていないドライブは起こさない）、
  Linux は `smartctl -H -A -n standby`（root か `disk` のグループが要る。無ければ理由を出す）、macOS は `diskutil info -plist` の `SMARTStatus`。
  温度は Windows が `StorageDeviceTemperatureProperty`（SSD は 5 秒ごと、ほかは 1 分の確認で）、Linux が hwmon、無ければ smartctl の値。
  右の列に「Health」と「Temperature」の行、壊れかけのドライブは左の名前も赤。hwmon のチップに無いドライブの温度は sensors の画面にも出る（v0.12.0）。
- [ ] disk: Windows で `IOCTL_STORAGE_PREDICT_FAILURE` に答えないドライブ（実機の 3d.8 で分かる）があれば、WMI の `MSFT_PhysicalDisk` の `HealthStatus` と `StorageReliabilityCounter`（管理者）を足す。
- [x] disk: macOS の NVMe のドライブの温度と SMART の中身（IOKit の `IONVMeSMARTInterface`）。
  → `NVMe SMART Capable` の付いた親の user client を `IOCreatePlugInInterfaceForService` で開き、`SMARTReadData` の 512 バイトを `disk::nvme_log` で読む。sudo は要らない（v0.14.0）。
- [x] disk: macOS の SATA のドライブの SMART の中身（`IOATASMARTInterface` の `SMARTReadData` と、温度は属性 194）。今は内蔵の SATA の Mac（古い Intel の Mac）と、SMART を通す外付けのケースだけが対象。
  → `SMART Capable` の付いた親の user client で `SMARTReturnStatus` と `SMARTReadData` を読み、属性の表は OS に依らない `disk::ata_attrs` で読む（Windows の `SMART_RCV_DRIVE_DATA` も同じ 512 バイト）。sudo は要らない（v0.15.0）。
- [x] disk: SMART の中身（使った寿命の %・代替処理済みのセクタ・電源を入れた時間）を右の列に。smartctl は `-j`、Windows は `IOCTL_STORAGE_QUERY_PROPERTY` の `StorageDeviceProtocolSpecificProperty`（NVMe の SMART ログ）。
  → 右の列の温度の下に「Life used」（80% から黄、100% で赤）・「Powered on」・「Lifetime writes」・「Remapped / Pending sectors」・「Media errors」（1 から黄）を、ドライブが答えた分だけ出す。
  Linux は `-j` ではなく今の `smartctl -H -A` の文字を読む（JSON を読むために serde_json を足さないため）。止まったドライブは前の値を残す。
  Windows は NVMe の SMART / health ログ（ページ 2）を読み、critical warning も故障の印にする。512 バイトのパースは OS に依らない `disk::nvme_log`（v0.13.0）。
- [x] disk: Windows の SATA の SMART の中身（`SMART_RCV_DRIVE_DATA` の属性。管理者が要る）。管理者でないときに何を出すかと合わせて決める。
  → 管理者なら属性を読み、そうでなければ「SMART counts: needs administrator」の 1 行を薄く出す（健康状態の行がすでに管理者が要ると言っていれば出さない）（v0.16.0）。
- [ ] disk の実機の確認 `【実機】`（TESTING.md 3d）。
- [x] sensors（`9`）: 温度・ファン（sysinfo の `Components`）。
  → 左がチップ（チップごとの一番熱い温度の 1 分のグラフ）、真ん中が温度とファンの一覧（今・1 分の最低と最高・高温の目安・限界）、右が選んだセンサーのグラフと目安の線。
  sysinfo の `Components` は macOS だけで使う。Linux は sysinfo がファンを読まずチップでまとめないので `/sys/class/hwmon` を自分で読み（温度が無ければ `/sys/class/thermal`）、
  Windows は sysinfo が管理者の要る WMI（`MSAcpi_ThermalZoneTemperature`）を使うので、管理者の要らない `Thermal Zone Information` のパフォーマンスカウンター（PDH）を読む。
  NVIDIA の GPU は NVML の温度とファンを足し、amdgpu のように OS が並べているものは PCI のアドレスで重ねない（v0.9.0）。
- [x] sensors: macOS のファンと、SMC のキーで読める温度の名前（`TC0P` などを「CPU proximity」に）。今は sysinfo の components の温度だけ。
  → `AppleSMC` を直に読み、名前の付いた温度とファン（最小・最大つき）を出す。Apple silicon の HID の温度にも名前（v0.17.0）
- [ ] sensors: Windows のファンとコアの温度。メーカーのドライバーか、LibreHardwareMonitor のようなカーネルドライバーが要る（署名・管理者・同梱の是非を QUESTIONS で決めてから）。今は ACPI の熱ゾーンだけ。
- [ ] sensors の実機の確認 `【実機】`（TESTING.md 3e）。
- [x] sys（`0`）: OS・BIOS・メモリのスロットなど、変わらない情報。
  Windows と Linux はファームウェアの SMBIOS の表を自分で読む（`smbios.rs`、純粋な関数）。Windows は `GetSystemFirmwareTable('RSMB')` で管理者が要らない。
  Linux は `/sys/firmware/dmi/tables/DMI` が root だけなので、ふだんは `/sys/class/dmi/id`（スロットなし、そう言う）、SMBIOS の無い ARM の板はデバイスツリーの `model`。
  macOS は `system_profiler -xml`（Apple シリコンはパッケージの中の 1 塊）。WMI（`Win32_PhysicalMemory`）は COM が重く遅いので使わない。
  起動時に 1 回、専用のスレッドで読む。シリアル番号は読んでも出さない（画面の写しを人に見せることがあるため）（v0.10.0）。
- [x] sys: CPU の型番・コア数・キャッシュ・ソケットを SMBIOS の type 4 と 7 から足す（今は sysinfo の CPU の名前だけ）。
  → 左の列に「Processor」の節。埋まったソケットの中央処理装置だけを読み、キャッシュはハンドルで type 7 を引く。macOS は system_profiler のチップ名とコア数（v0.18.0）
- [x] sys: GPU とドライブの一覧への近道（gpu·npu・disk の画面へ飛ぶ）を置くか決める。
  → 新しいキーは足さず、左の列に「Graphics」「Drives」の節（名前・VRAM、型番・大きさ）を置き、末尾にその画面を開く keymap のキーを出す（v0.19.0）
- [ ] sys の実機の確認 `【実機】`（TESTING.md 3f）。

## 2. 構成と OS

- [x] macOS のコアの構成: `sysctl hw.perflevelN.*`（P / E コアと L2 の塊）。今は 1 論理 CPU = 1 コアで描く。
  → Apple silicon は IOKit のデバイスツリー（`cpus` の子の `cluster-type`・`cluster-id`）でクラスターごとの塊、L2 の大きさは sysctl。Intel の Mac は sysctl のコア数とスレッド数（v0.20.0）
- [ ] Windows でプロセスのユーザー名が出るか確かめる `【実機】`（TESTING.md 3.x）。
- [ ] Windows ARM64（Snapdragon）の構成: `EfficiencyClass` が P / E を分けるか `【実機】`。
- [ ] AMD の GL ドライバーで落ちる件（filer の TODO と同じ `atio6axx.dll`）。filer で原因が分かったら同じ対策を入れる。今は `config.toml` の `backend = "dx12"` で避けられる。
- [x] macOS の GPU: IOKit の `IOAccelerator` の `PerformanceStatistics`（使用率・VRAM）。Pure Rust の `objc2-io-kit` で読めるか確かめる。
  → `objc2-io-kit` で読める。Apple silicon は使用率・Renderer / Tiler・使用中の統合メモリ、AMD は VRAM とセンサーも（v0.21.0）
- [ ] Windows の GPU のセンサー（温度・クロック）を NVIDIA 以外でも: D3DKMT の `D3DKMTQueryAdapterInfo`（Task Manager の温度の出どころ）。
- [ ] Linux の Intel GPU の使用率: `/proc/<pid>/fdinfo` の `drm-engine-*`（i915 / xe が出す）から、全体とプロセスごとの使用率を出す。amdgpu も同じ形でプロセスごとが取れる。
- [ ] NVML のプロセスごとの使用率（`process_utilization_stats`）。今はメモリだけ。
- [ ] GPU·NPU の実機の確認 `【実機】`（TESTING.md 3a）。

## 3. 言語

- [ ] 日本語の表（`src/lang/ja.toml`）と CJK のフォント（tsumugi の `fonts.rs` の取り方）。
- [ ] `i18n` と `common.toml` の読み込みを tsumugi の workspace の共有クレートに切り出し、filer と tsumugi から使う（tsumugi を `access:"push"` で付け直す）。
  tsumugi の設定画面の言語の欄もそれにつなぐ。
- [ ] keymap の読み込み（`config/keys.rs`・`keymap.rs`）も共有のクレートにするか、filer と見比べて決める。

## 4. リモート

- [ ] 見本の「設計」タブのとおり、ほかの機械のエージェント（同じバイナリの `--agent`）から `Msg` を受けて fleet の画面に並べる。
  通信の形（TLS と鍵の置き方）は QUESTIONS.md で決めてから。

## 5. 配布

- [ ] リリースの workflow（6 ターゲット: Windows / macOS / Linux × x86_64 / ARM64）。filer の `release.yml` と `release-sums.yml` に倣う。
- [ ] Windows のアイコンと版の情報（`winres` は C を要らない形で）。
- [ ] CI に Linux のスモーク（filer の `smoke` と同じく lavapipe で起動して撮る）。
