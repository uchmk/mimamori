# README の画像の置き場

README の「What it looks like」と、紹介画像（GitHub の Social preview）の右に入れる実画面の置き場。
どれもリリースのビルドを既定のテーマで撮った。

| ファイル | 何を写すか | 撮り方 |
| --- | --- | --- |
| `cpu.png` | cpu の画面（`2`）。コアのタイルが何色かに分かれているところ | 窓を 2000x1150 にし、いくつかのスレッドを忙しくして撮る（全部が赤や全部が青だと色の意味が伝わらない） |
| `overview.png` | overview の画面（`1`） | 窓を 2000x1150 にして撮る |
| `yagura.gif` | 数字のキーで画面を回る動き。押したキーを右上に字幕で出す | 窓を 1600x1000 にして ffmpeg の `gdigrab`（15 fps、マウスなし）で撮り、幅 1200 px・10 fps・128 色の GIF にする |
| `banner-shot.png` | `docs/banner.html` の右の実画面。cpu の画面 | 窓を 1700x1100 にして撮る。狭いとタイルが折り返し、2 つめの L3 の組が欄から出てしまう |

撮るときの注意:

- 写るものに外に出したくないもの（ユーザー名の入ったパス、プロセスの引数、社内の名前）が無いかを見る。
  タブの列の右にはホスト名が出るので、出してよい名前の機械で撮る。
- 紹介画像は `docs/social-preview.png`（元は `docs/banner.html`。描き直し方は HTML の冒頭）。
  GitHub に API が無いので、Settings > General > Social preview から手で上げる。
