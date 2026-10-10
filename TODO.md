# TODO

v0.1.0 で overview・cpu・mem・procs の 4 枚ができた。残りは下の順で進める。
`【人】` は持ち主の作業、`【実機】` は実機でしか確かめられないもの（[TESTING.md](TESTING.md) に積む）。

## 1. 残りの画面（番号は v0.1 で固定済み）

- [x] procs: プロセスの終了（`x`、確認のオーバーレイを挟む。Windows は `TerminateProcess`、ほかは SIGTERM → SIGKILL）。
- [ ] procs: 木の表示（親子）と、スコープ（All / Mine）の切り替えのキー。
- [ ] gpu·npu（`4`）: 使用率・VRAM・温度。Windows は PDH の `GPU Engine` カウンタ、Linux は `/sys/class/drm` と amdgpu / i915 の sysfs、NVIDIA は NVML を動的に読む（無ければ出さない）。NPU は見本の「設計」タブの限界のとおり、取れる OS だけ。
- [ ] svc（`6`）: Windows のサービス、Linux の systemd のユニット、macOS の launchd。読むだけから始める。
- [ ] net（`7`）: インターフェースごとの送受信のグラフ、接続の一覧。
- [ ] disk（`8`）: ドライブごとの使用量と読み書きの速さ。
- [ ] sensors（`9`）: 温度・ファン（sysinfo の `Components`）。
- [ ] sys（`0`）: OS・BIOS・メモリのスロットなど、変わらない情報。

## 2. 構成と OS

- [ ] macOS のコアの構成: `sysctl hw.perflevelN.*`（P / E コアと L2 の塊）。今は 1 論理 CPU = 1 コアで描く。
- [ ] Windows でプロセスのユーザー名が出るか確かめる `【実機】`（TESTING.md 3.x）。
- [ ] Windows ARM64（Snapdragon）の構成: `EfficiencyClass` が P / E を分けるか `【実機】`。
- [ ] AMD の GL ドライバーで落ちる件（filer の TODO と同じ `atio6axx.dll`）。filer で原因が分かったら同じ対策を入れる。今は `config.toml` の `backend = "dx12"` で避けられる。

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
