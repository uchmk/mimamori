# 変更履歴

このファイルの形式は [Keep a Changelog](https://keepachangelog.com/ja/1.1.0/) に、版のつけ方は
[セマンティックバージョニング](https://semver.org/lang/ja/) に従う。版は `Cargo.toml` の
`version` が正。手順は [CLAUDE.md](CLAUDE.md) の「版・変更ログ・push」を参照。

## [0.33.1] - 2026-10-11

### 修正

- 改名の移行で、古いフォルダー `uchmk/mimamori/` を何かが掴んでいると（Explorer・エディター・そこに cd した端末・動いている
  mimamori.exe や `mimamori --agent`）、Windows で移せず（os error 32）、毎回既定の設定で起動していた。移せないときは中身を
  `uchmk/yagura/` に複写して、そちらを読む。古いフォルダーは残すので、掴んでいるものを閉じてから消してよい（設定の画面の Advanced に
  そう出る）。複写もできなかったときは、その回は古いフォルダーを読む。
- README の「Building」に、ビルドの出口をその機械だけ別の場所（RAM ディスクなど）にする書き方を足した（clone の
  `.cargo/config.toml`。git には入らない）。

## [0.33.0] - 2026-10-11

### 変更

- 名前を mimamori から yagura（櫓）に変えた。リポジトリは [uchmk/yagura](https://github.com/uchmk/yagura)。
  バイナリ（`yagura`、`yagura --agent`）・窓のタイトル・アプリの ID・環境変数（`YAGURA_KEYS_DONE`、`YAGURA_BIN`）・
  リリースの成果物（`yagura-vX.Y.Z-*`）・スレッドの名前が新しい名前になる。姉妹の filer は kura と呼ぶ。
- 設定のフォルダーを `<config>/uchmk/mimamori/` から `<config>/uchmk/yagura/` に変えた。最初の起動で、古いフォルダーだけがあれば
  新しい名前に移す（config.toml・keymap.toml・state.toml がそのまま残る）。両方あれば新しいほうを使い、古いほうには触らない。
  移せなかったときは設定の画面の Advanced に警告を出す。
- 【人】タスクバーのピン留め・ショートカット・スクリプトで `mimamori.exe` を指していたら `yagura.exe` に直す。
  ほかの機械で `mimamori --agent` を動かしていたら、新しい版の `yagura --agent` に替える（つなぎ方は変わらない）。

## [0.32.1] - 2026-10-11

### 変更

- 共有のクレートを [uchmk/ito](https://github.com/uchmk/ito) から読むようにした（`tsumugi-{layout,i18n,keys,common,theme,prefs}` → `ito-*`、
  ito 0.2.0 の `90314f9`）。中身は今までの `rev`（tsumugi v0.81.0 の `a26aefd`）と同じで、動きは変わらない。
  共通仕様の参照先を ito の `docs/common-spec.md` に直した。

## [0.32.0] - 2026-10-11

### 追加

- 設定の画面（`Ctrl+,`、macOS は `Cmd+,`）。tsumugi と同じレイアウトで窓いっぱいに出す（左に検索・ページ・「Open config.toml」、右にページの題とカード）。
  - General: 言語（common.toml）・更新の間隔（すぐ効く）・時計のカード（common.toml）。
  - Theme: Follow OS / Light / Dark・テーマの一覧・mimamori のタイルとプロセスの行とステータスバーで描く見本（common.toml）。
  - Fleet: この機械のエージェント（プロセスの終了・サービスの操作を許すスイッチ、ポート）、`[[host]]` の一覧（名前・宛先・Delete・Add host）、
    つなぎ方の行（`mimamori --agent`・ssh・tailscale・`[[host]]` の見本・config.toml のパス）と Copy のボタン。ホストを足すと再起動なしでつなぐ。
  - Advanced: 描画のバックエンド（次の起動から）、設定のフォルダー（Copy・Open）、common.toml のパス、読み込みの警告。
  - 書き込みは toml_edit でコメントを残し、UI ではないスレッドで行う。画面の中では `[settings]` の節のキーだけを取り（閉じる・ページを移る・検索）、
    ほかのキーは部品に渡す（欄に `q` や `3` を打っても終わったり画面が変わったりしない）。
- テーマ。tsumugi と同じ 13 の組み込み（tsumugi Dark / Light、Tokyo Night、Catppuccin Mocha / Latte、Dracula、Nord、Gruvbox Dark / Light、
  Solarized Dark / Light、One Dark、Rosé Pine）と、`<config>/uchmk/themes/*.toml` の自分のテーマ。明るいテーマでは egui の部品も明るくなる。
- ステータスバーの右端に日付と時刻（tsumugi と同じ場所・字体・色・書式）。分が変わるときだけ再描画し、ポインタを載せると日付を全部出す。日本語では曜日を「(土)」と出す。
- コピー。`y`（fleet の手引きでは `[[host]]` の見本、ほかの機械のカードではその宛先）、手引きの水色の行のクリック、設定の Copy のボタン。トーストで「Copied: …」と言う。
  手引きの頭に「Ctrl+, で設定が開く」を添えた。
- キーマップの `settings`・`copy`・`page next` / `page prev`・`search` のコマンドと、`[settings]` の節。

### 変更

- テーマと時計の設定は `<config>/uchmk/common.toml`（`theme`・`dark_theme`・`light_theme`・`[clock]`）に置く。uchmk の全部のアプリが読む共通の設定で、
  このファイルを変えれば起動中のアプリも 2 秒のうちに一括で変わる（言語も、起動中に切り替わるようになった）。置き場所と画面の約束は tsumugi の `docs/common-spec.md`。
- 既定のテーマ「tsumugi Dark」で今までの見た目をほぼ保つが、薄い文字（DIM）が少し明るくなった。

## [0.31.0] - 2026-10-10

### 追加

- fleet の画面（`F`）。この機械と、`config.toml` の `[[host]]` ごとのカードを並べる。カードには状態の点（緑: つながっている、黄: 遅い、
  赤: 落ちた、灰: つなぎ中）・名前・宛先と往復の時間・OS と CPU・CPU / メモリ / GPU のゲージと直近 1 分・温度・稼働時間（落ちていれば最後に見た時刻）・
  エージェントの版と許していることを出す。落ちたカードは暗くし、最後の数字を残して理由（トンネルが無い・エージェントが動いていない・
  プロトコルが違う・ほかのものが答えた）を出す。`h` `j` `k` `l` と矢印・`gg` / `G`・クリックでカードを選び、この機械のカードで `Enter` を押すと画面に戻る。
  見出しに `F fleet` のタブ、状態の欄に `FLEET`（選んだ機械が落ちていれば `STALE`）と `n/m`。`[[host]]` が無ければ、つなぎ方の手引きを出す。
- `mimamori --agent`。窓を出さずに同じ収集を回し（プロセスの一覧は除く）、ラウンドごとの要約（CPU・メモリ・GPU・温度）を、
  `127.0.0.1:7313` だけで待ち受けて見る側に送る。暗号も認証も持たず、`ssh -L` か `tailscale serve` の上で使う（Q1 の 1）。
  送り方は postcard（serde の小さなバイナリ。Pure Rust、新しい依存はこれだけ）とプロトコルの番号。見る側はホストごとのスレッドで 5 秒ごとに ping し、
  200 ms を超えるか要約が 3 回来なければ遅いとし、落ちたら 2 秒から 30 秒まで待ちを延ばしてつなぎ直す。
  Windows ではコンソールから起動すれば、`--agent`・`--help`・`--version` の出力がそのコンソールに出る。
- `config.toml` の `[agent]`（`port`、`allow = ["kill", "service"]`）と `[[host]]`（`name`・`addr`）。`allow` は読んでカードに出すが、
  ほかの機械での操作は後の版（今は見るだけ）。
- キーマップの `fleet` のコマンドと、既定の `F`。

## [0.30.0] - 2026-10-10

### 追加

- sensors（Windows）: HWiNFO が共有メモリを有効にして動いていれば、`Global\HWiNFO_SENS_SM2` から全部のチップ（CPU・板・ファン・ドライブ・GPU）の
  温度とファンを読む（`src/collect/sensors/hwinfo.rs`。読むたびに開いてコピーして閉じるので、HWiNFO を終えれば消える）。°F の値は °C に直す。
- sensors（Windows）: HWiNFO が無く LibreHardwareMonitor（か OpenHardwareMonitor）が動いていれば、WMI の `root\LibreHardwareMonitor` の
  `Sensor` と `Hardware` から読む（`src/collect/sensors/lhm.rs`。つながらなければ 10 秒ごとに探し直す）。どちらもドライバーは同梱せず、管理者も要らない。
  出どころの行は `HWiNFO: …` / `LibreHardwareMonitor …`。
- ツールが挙げた GPU とドライブは、D3DKMT・NVML・ディスクの読み値から重ねて足さない。

### 変更

- Windows の熱ゾーンだけのときの注意書きに、ファンとコアを読むには HWiNFO か LibreHardwareMonitor を動かす、と書く。

## [0.29.1] - 2026-10-10

### 追加

- CI に Linux のスモーク（`smoke` のジョブ、`scripts/smoke-linux.sh`）。Xvfb と Mesa のソフトウェア OpenGL で起動し、1〜0 の画面を順に撮って、
  panic・状態が別の画面・何も描かれていない画像のどれかで落ちる。画像と状態とログは成果物 `linux-smoke` に残る。

## [0.29.0] - 2026-10-10

### 追加

- アイコン。濃い角丸の板に 4 つのコアのタイル（下から使用率の色で満たす、cpu の画面と同じ描き方）を、コードで描く（`src/icon.rs`）。
  窓のアイコン（タイトルバー・Alt+Tab・タスクバー・Dock）に使う。
- Windows の `mimamori.exe` にアイコンと版の情報（説明・製品名・版）。`build.rs` が `.res` を Rust で書いてリンカーに渡すので、
  rc.exe も C のツールも要らず、Linux や macOS からの Windows 向けのビルドにも付く。エクスプローラー・ファイルのプロパティの「詳細」・
  タスク マネージャーに出る。

## [0.28.0] - 2026-10-10

### 追加

- リリースの workflow（`release.yml`）。Actions タブから `tag` に `vX.Y.Z` を渡して回すと、Windows（`.zip`）・macOS・Linux（`.tar.gz`）の
  x86_64 と ARM64 の 6 つを作ってリリースに付ける。ノートは前のタグからの `vX.Y.Z:` のコミットの最初の段落から作り、
  最後に `release-sums.yml` が各ファイルと zip の中身の SHA-256 の表を足す。README に「Downloading」の節を足した。

## [0.27.1] - 2026-10-10

### 追加

- QUESTIONS.md（持ち主への確認事項）。Q1 は fleet のエージェントとの通信の形（SSH / Tailscale だけ・rustls・Noise）、
  Q2 は Windows のファンとコアの温度の読み方（HWiNFO と LibreHardwareMonitor を読む・PawnIO・今のまま）。

## [0.27.0] - 2026-10-10

### 変更

- キーの書き方（`<C-a>`・`<A-S-Up>`・`<P-e>`）の読み取り・表示と、egui のキーの出来事の変換を、tsumugi の workspace の共有クレート
  `tsumugi-keys` に移した。キーの意味は変わらない。keymap の層（`[main]` など）とコマンドは mimamori に残す。

## [0.26.0] - 2026-10-10

### 変更

- 言語の表・言語の決め方・`common.toml` の読み方を、tsumugi の workspace の共有クレート `tsumugi-i18n` に移した。
  tsumugi の設定画面（General → Language）で選んだ言語が `common.toml` に書かれ、mimamori も次の起動からそれに従う。
  `tsumugi-layout` も同じ `rev` に上げた（中身は変わっていない）。
- `common.toml` が読めないとき（`language` が文字列でない、TOML でない）は警告に出し、言語は OS のものになる。

## [0.25.0] - 2026-10-10

### 追加

- 日本語の表（`src/lang/ja.toml`）。`common.toml` か `config.toml` の `language = "ja"`、または `auto` で OS が日本語のときに使う。
  `?` のヘルプのキーの説明も、既定の keymap のものは訳す（自分で書いた説明はそのまま）。
- CJK のフォント: OS のフォント（Windows は BIZ UDゴシック・MS ゴシック・游ゴシック・メイリオ、macOS はヒラギノ、
  Linux は Noto Sans CJK・IPA ゴシック）を起動時にスレッドで読み、組み込みのフォントの後ろに足す。英語のときも、
  日本語のプロセス名やサービスの説明が豆腐にならない。

### 修正

- 幅に合わせて文字を切るとき、全角の文字を 2 桁で数えるようにした（CJK の名前が隣の列にはみ出していた）。
  gpu·npu の画面の説明の折り返しは、空白の無い日本語でも文字の間で折り返す。

## [0.24.0] - 2026-10-10

### 追加

- gpu·npu の画面（Linux、NVIDIA）: GPU を使っているプロセスの使用率も出すようにした（今まではメモリだけ）。NVML の
  `nvmlDeviceGetProcessUtilization` の標本を、前の回からの分だけ SM・エンコーダー・デコーダーの大きいほうで平均する。
  一覧は使用率の高い順に並ぶ。Maxwell 以降の GPU で取れる。
- Windows では NVML からプロセスの一覧を取らないようにした（使わずに捨てていた。プロセスはカウンタから出る）。

## [0.23.0] - 2026-10-10

### 追加

- gpu·npu の画面（Linux）: DRM の fdinfo（`/proc/<pid>/fdinfo`）から、GPU を使っているプロセスとその使用率・VRAM を出すようにした。
  i915・xe・amdgpu・panfrost・msm などが書く形を読む。
  - Intel の iGPU（i915 / xe）や Arm の GPU のように sysfs に使用率が無いものは、全体の使用率とエンジン（Render・Video・Copy…）も fdinfo の合計から出す。
    読めるのは自分のプロセスだけなので、全部を見るには root で動かす。
  - `/dev/dri` を開いている fd は 5 秒ごとに探し直し、その間は見つけた fd だけを読む。

### 修正

- sensors の画面（Linux）: AMD の GPU の温度が、`amdgpu` のチップと GPU の名前の行の 2 回出ていた。PCI のアドレスで重なりを見るところで、
  GPU の側が PCI の ID（`1002:744c`）を渡していた。

## [0.22.0] - 2026-10-10

### 追加

- gpu·npu の画面（Windows）: NVIDIA 以外の GPU でも温度・ファン・クロックを出すようにした。D3DKMT の `D3DKMTQueryAdapterInfo`
  （タスク マネージャーの GPU の温度の出どころ）から読む。ドライバーが持たない値は今までどおり理由を出す。電力は出ない。
  - NVIDIA の GPU は NVML の値が優先で、NVML が無いときも温度とクロックは D3DKMT から出る。
  - GPU の温度は sensors の画面にも出る。

## [0.21.0] - 2026-10-10

### 追加

- gpu·npu の画面（macOS）: GPU を IOKit の `IOAccelerator` から読むようにした。今までは「まだ作っていない」と出していた。
  - `PerformanceStatistics` から使用率（`Device Utilization %`、AMD は `GPU Activity(%)`）、Apple silicon の Renderer と Tiler、
    使用中のメモリ（Apple silicon は統合メモリなので「Shared」）、AMD のカードの VRAM・温度・ファン・電力・クロックを出す。
  - 名前は Apple silicon がアクセラレーターの `model`（`Apple M2 Pro`）、Intel の Mac の GPU は PCI の装置の `model` と ID。
  - 取れない値は理由を出す（統計に無い・センサーが無い。SMC の GPU の温度は sensors の画面にある）。プロセスごとの使用は無い。

## [0.20.0] - 2026-10-10

### 追加

- cpu の画面（macOS）: コアの構成を読むようにした。今までは 1 論理 CPU を 1 コアとして `CPU` の 1 つの塊に描いていた。
  - Apple silicon は IOKit のデバイスツリー（`IODeviceTree:/cpus` の子の `cluster-type`・`cluster-id`・`logical-cpu-id`）から、
    クラスター（L2 を共有する P コアか E コアの塊）ごとに塊を作る。L3 は無いので塊の見出しは L2 の大きさ（`sysctl hw.perflevelN.l2cachesize`）。
  - Intel の Mac は `hw.physicalcpu` / `hw.logicalcpu` でスレッドを隣どうしに組み、`hw.l3cachesize` を見出しに出す。
  - `Topology from` は `iokit` か `sysctl`。塊の見出しの文字の鍵は `tiles.l3` から `tiles.cache`（`L{0} {1}`）に変えた。

## [0.19.0] - 2026-10-10

### 追加

- sys の画面: 左の列に「Graphics」（GPU と NPU の名前と VRAM）と「Drives」（ドライブの型番か名前と大きさ）の節を足した。
  それぞれの末尾に、その画面（gpu·npu・disk）を開くキーを keymap から引いて「Press 4」のように出す。新しいキーは足していない。

## [0.18.0] - 2026-10-10

### 追加

- sys の画面: 左の列に「Processor」の節を足した。型番・メーカー・ソケット・コア数・スレッド数・最高のクロック・L1 / L2 / L3 のキャッシュを出す。
  - Windows と Linux（root のとき）は SMBIOS の type 4（埋まったソケットの中央処理装置だけ）と、そこから引く type 7 のキャッシュを読む。
    256 を超えるコア数の拡張の欄と、64 KiB 単位・拡張のキャッシュの大きさも読む。ソケットが 2 つ以上なら「2 × ソケット」と、全部のコア数を足して出す。
  - macOS は `system_profiler` のチップ名（`Apple M2 Pro`）か CPU の名前と、コア数。Intel の Mac は L3 も。
  - ファームウェアが言わないときは、コア数とスレッド数をコアの構成から出す。

### 変更

- sys の画面: CPU の名前を「System」の節から「Processor」の節の「Model」に移した。

## [0.17.0] - 2026-10-10

### 追加

- sensors の画面（macOS）: `AppleSMC` を直に読み、ファンを出す。
  - 「Fan 1」… を rpm で出し、最小（`F0Mn`）と最大（`F0Mx`）も出す。最小を下回れば赤くなる。
  - SMC の温度に名前を付ける。Intel の Mac は知られたキー（`TC0P` は「CPU proximity」、`TC1C` は「CPU core 1」など）、
    Apple silicon は `Tp…`・`Te…`・`Tg…`・`TH…` の系列をキーの順に「CPU 1」「CPU efficiency 1」「GPU 1」「SSD 1」と数える。名前の無いキーと、0 や 130 °C 以上を読むキーは出さない。
  - キーは起動のときに 1 回だけ並べ、毎回は残したキーだけを読む。読んだ所に SMC のキーを出す。

### 変更

- sensors の画面（macOS）: Apple silicon の HID の温度を「SMC」から分けて「HID sensors」のチップに置き、名前を読みやすくした
  （「PMU tdie3」→「CPU die 3」、「PMU tdev1」→「SoC 1」、「NAND CH0 temp」→「SSD」、「gas gauge battery」→「Battery」）。温度ではない「PMU tcal」は出さない。
  Intel の Mac では sysinfo が読む SMC の温度を SMC の読み値と重ねない。

## [0.16.0] - 2026-10-10

### 追加

- disk の画面（Windows）: SATA のドライブの SMART の中身を出す。
  - `SMART_RCV_DRIVE_DATA`（SMART READ DATA）で属性の表を読み、右の列に「Powered on」「Remapped sectors」「Pending sectors」と、SSD なら「Life used」を出す。
    `StorageDeviceTemperatureProperty` が温度を返さないドライブは属性 194 の温度を使う。読むだけで、SMART を有効にする・自己診断を走らせることはしない。
  - Windows はこの問い合わせに管理者にしか答えない。管理者でないときは「SMART counts: needs administrator」の 1 行を薄く出す
    （健康状態の行がすでに管理者が要ると言っていれば出さない）。

### 修正

- CI の新しい clippy（`chunks_exact_to_as_chunks`）で v0.15.0 の lint が落ちていたのを直した。

## [0.15.0] - 2026-10-10

### 追加

- disk の画面（macOS）: SATA のドライブ（古い Intel の Mac の内蔵のドライブと、SMART を通す外付けのケース）の温度と SMART の中身を出す。
  - IOKit の `IOATASMARTInterface`（`SMART Capable` の付いた装置の user client）で `SMARTReturnStatus` と `SMARTReadData` を読む。sudo は要らない。
    SMART を有効にする・自己診断を走らせるといった書き込みはしない。
  - 右の列に「Temperature」（属性 194、無ければ 190）「Powered on」「Remapped sectors」「Pending sectors」と、SSD なら「Life used」が出る。
  - しきい値を超えた属性があれば故障の印にする。

### 変更

- 属性の表（512 バイト）のパースを OS に依らない `disk::ata_attrs` にし、ログを健康状態に足す処理を `Check::add_log`（`add_nvme` から改名し、温度の出どころを渡す）にした。

## [0.14.0] - 2026-10-10

### 追加

- disk の画面（macOS）: NVMe のドライブ（Mac の内蔵の SSD と、SMART を通す外付けのケース）の温度と SMART の中身を出す。
  - IOKit の `IONVMeSMARTInterface`（`NVMe SMART Capable` の付いた装置の user client）で SMART / health ログを読む。sudo は要らない。
  - 右の列に「Temperature」「Life used」「Powered on」「Lifetime writes」「Media errors」が出る。温度は sensors の画面にも出る。
  - ログの critical warning も故障の印にする。`diskutil` が SMART を言わないドライブでも、ログが読めれば健康状態が出る。
  - SATA のドライブの中身はまだ読まない。

### 変更

- NVMe のログを健康状態に足す処理を `Check::add_nvme` にまとめ、Windows と macOS で同じものを使う（動きは変わらない）。

## [0.13.0] - 2026-10-10

### 追加

- disk の画面: 右の列の温度の下に、ドライブの SMART の中身を出す。ドライブが答えた分だけ並べる。
  - 「Life used」（使った寿命の %。80% から黄、100% で赤）・「Powered on」（電源を入れた時間と日数）・「Lifetime writes」（これまでに書いた量）。
  - 「Remapped sectors」（代替処理済み）・「Pending sectors」（代替待ち）・「Media errors」。1 以上で黄。
  - Windows: NVMe のドライブは `IOCTL_STORAGE_QUERY_PROPERTY` の `StorageDeviceProtocolSpecificProperty` で SMART / health ログを読む。
    ログの critical warning も故障の印にし、温度の読めないドライブはログの温度を使う。SATA の中身はまだ読まない（管理者が要る）。
  - Linux: 健康状態と同じ `smartctl -H -A -n standby` の出力から読む（NVMe の Percentage Used など、ATA の属性 5・9・197 と寿命の属性、SCSI の値）。
    TODO にあった `-j` の JSON ではなく文字を読む（JSON のために serde_json を足さないため）。
  - 止まっていて起こさなかったドライブは、前の回の値を残す。

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
