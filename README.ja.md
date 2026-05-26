# agent-lx-music

[English](README.md) | [简体中文](README.zh-CN.md) | [繁體中文](README.zh-TW.md) | [日本語](README.ja.md) | [Français](README.fr.md) | [Español](README.es.md)

Unixの哲学に基づいて設計された、Rust製の高性能コマンドラインミュージックプレイヤー。lx-musicの音源スクリプトと完全な互換性があります。肥大化したElectronフレームワークを完全に排除し、高度に最適化されたQuickJSサンドボックス環境（`rquickjs`）でスクリプトを実行。さらに、POSIXの独立したバックグラウンドプロセスグループ（`setsid`）を通じて、ヘッドレスの`mpv`インスタンスに高音質のデコードと再生処理を委託するアーキテクチャを採用しています。

---

## 主な特徴

- **QuickJS分離サンドボックス**：従来の`lx-music`音源解析スクリプトを、[rquickjs](https://github.com/DelSkayn/rquickjs)を使用した安全で隔離されたサンドボックス環境内で高速実行します。
- **POSIXデーモン設計**：`setsid`メカニズムを利用して、独立したバックグラウンドプロセスで`mpv`を起動。コマンドラインを閉じても音楽再生が停止しない、ノンブロッキングな制御フローを実現。
- **SQLite透過的データベースキャッシュ**：プレイリスト、自動削除に対応した再生履歴、お気に入りをローカルに保存。解析済みの歌詞（LRC）をローカルにキャッシュすることで、2回目以降の再生をネットワークアクセスなしで即座に開始します。
- **歌詞とカバー画像処理**：LRC形式の主歌詞、翻訳歌詞、ローマ字表記トラックの高速なコンソール出力およびエクスポートに対応。Magic Bytes検出により、不安定なMIMEヘッダーを回避し、画像ファイルの拡張子を自動で正確に補完します。
- **コンテナ配備対応**：rootlessのPodman / Dockerと深く互換性があり、ボリュームマウントを通じてホストのPulseAudio/Pipewireサウンドサーバーに直接オーディオパススルーが可能です。
- **AIエージェント対応**：XDG仕様に準拠したAIスキルファイル（`music-discovery`、`audio-analysis`、`listening-companion`）を内蔵。Gemini 1.5 Proなどの多モ態大規模言語モデルと連携し、楽曲分析や音楽対話を行うことができます。

---

## インストール方法

ソースコードからビルドする場合（Rustツールチェーンが必要です）：

```bash
# リポジトリをクローン
git clone https://github.com/Xuepoo/agent-lx-music.git
cd agent-lx-music

# リリースビルドを実行
cargo build --release

# グローバルヘルプを表示
./target/release/alx --help
```

---

## クイックコマンドリファレンス

```bash
# 1. 音楽解析スクリプトを登録
alx source add ./my-sixyin-source.js

# 2. 曲を検索（動的な短いCLI IDが返されます）
alx search "周杰伦 晴天"

# 3. バックグラウンドのmpvデーモン経由で再生開始
alx play <cli_id>

# 4. 音楽再生を非同期でコントロール
alx now                    # リアルタイムの進捗カードを表示
alx volume +10 / alx volume -10
alx seek +30 / alx seek 2:30
alx pause / alx resume / alx stop
alx quit                   # mpvデーモンを完全にクリーンに終了します

# 5. LRC歌詞とカバー画像の保存
alx lyric <cli_id>         # 同期したLRC歌詞を表示
alx lyric <cli_id> --save  # ダウンロードフォルダに .lrc ファイルとして保存
alx pic <cli_id> --save    # 拡張子自動検出でアルバムカバー画像を保存
```

---

## 技術ドキュメント案内

詳細な設計仕様、API契約、データモデルはすべて`docs`ディレクトリに格納されています：
- [要件定義書](docs/REQUIREMENTS.md) — 詳細な機能仕様とロードマップ
- [技術アーキテクチャ](docs/ARCHITECTURE.md) — モジュール分割とmpv IPC設計
- [CLIリファレンス](docs/CLI.md) — 全コマンドとオプションの解説
- [音源ブリッジAPI仕様](docs/SOURCE-API.md) — サンドボックス実行時のイベント契約
- [XDGパス設定ガイド](docs/CONFIG.md) — 環境変数とファイルシステム設計
- [SQLiteデータモデル](docs/DATA-MODEL.md) — データベース設計

---

## ライセンス

本プロジェクトはMITライセンスのもとで公開されています。
