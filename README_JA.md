<div align="center">
  <img src="./app/public/logo.svg" width="128" height="128" alt="Shanten Lens Logo" />
  <h1>Shanten Lens</h1>
</div>

Shanten Lens は、『雀魂』の青雲の志モード向けデスクトップ補助ツールです。ゲーム状態をローカルで解析し、お守りとショップ情報、手牌と牌山の分析、交換ガイド、スコア予測、自動化、ヒューズ保護、プラグイン拡張、デバッグ機能を提供します。

<div align="center">
  <a href="https://github.com/SpCoGov/shanten-lens/stargazers"><img src="https://img.shields.io/github/stars/SpCoGov/shanten-lens?logo=github" alt="GitHub stars" /></a>
  <a href="https://github.com/SpCoGov/shanten-lens/releases"><img src="https://img.shields.io/github/v/release/SpCoGov/shanten-lens?label=release&logo=github&include_prereleases" alt="Latest release" /></a>
  <a href="https://github.com/SpCoGov/shanten-lens/issues"><img src="https://img.shields.io/github/issues/SpCoGov/shanten-lens?logo=github" alt="Open issues" /></a>
  <a href="./LICENSE"><img src="https://img.shields.io/badge/license-Apache%202.0-blue?logo=apache" alt="License: Apache-2.0" /></a>
  <a href="https://github.com/SpCoGov/shanten-lens/actions/workflows/release-build.yml"><img src="https://img.shields.io/github/actions/workflow/status/SpCoGov/shanten-lens/release-build.yml?branch=v3&logo=githubactions&label=build" alt="Build status" /></a>
  <a href="https://tauri.app/"><img src="https://img.shields.io/badge/Tauri-2-24C8DB?logo=tauri&logoColor=white" alt="Tauri 2" /></a>
  <a href="https://www.rust-lang.org/"><img src="https://img.shields.io/badge/Rust-2021-000000?logo=rust&logoColor=white" alt="Rust 2021" /></a>
  <a href="https://react.dev/"><img src="https://img.shields.io/badge/React-18-61DAFB?logo=react&logoColor=black" alt="React 18" /></a>
</div>

[中文 README](./README.md)

## 目次

- [利用上の注意](#利用上の注意)
- [機能](#機能)
- [使用方法](#使用方法)
- [技術スタック](#技術スタック)
- [ディレクトリ構成](#ディレクトリ構成)
- [ビルドとパッケージ化](#ビルドとパッケージ化)
- [プロジェクトの検証](#プロジェクトの検証)
- [プラグイン](#プラグイン)
- [よくある質問](#よくある質問)
- [ライセンス](#ライセンス)

## 利用上の注意

本プロジェクトは教育および研究のみを目的としています。サードパーティ製の補助ツール、自動化、通信内容の変更はゲームの利用規約に違反し、アカウント制限の対象となる可能性があります。利用者自身で関連するリスクを確認し、その責任を負ってください。

現在、最も安定して利用できる環境は Windows です。通常の使用には、次のものが必要です。

- 正常に動作する Shanten Lens デスクトップアプリ
- `Proxifier`、またはプロセス単位でプロキシ転送できる同等のツール
- ローカルルート証明書をインストールする権限
- プロキシツールの対象にできる雀魂クライアントまたはブラウザプロセス

初回利用時は、Shanten Lens を一度起動してから終了し、設定、証明書、データ用のディレクトリを生成することを推奨します。

## 機能

- メイン画面でリアルタイム表示：
  - 現在のお守りとスタンプ
  - ショップの商品と候補のお守り
  - 手牌、牌山、交換牌の山
  - 打牌候補と牌山統計
  - 現在のスコア、目標スコア、ステージ情報
- スコア計算機：
  - 現在のお守りの順序に従ってスコアをリアルタイム計算
  - お守りごとの発動回数、追加発動、成長式を編集
  - 今後のステージのスコアを予測
  - 伝導スタンプや後続の成長などの計算ルールに対応
- 花火ガイドと交換ガイド：
  - 花火の交換プランを検索
  - 候補プラン、4カン一覧、検索対象の牌を表示
  - 万象交換プランを計算
  - 手順ごとの実行、または交換プランの一括実行
- 自動化：
  - 目標のお守りとスタンプに基づいて青雲の志を繰り返し実行
  - 再開記録、候補の詳細、操作ログを表示
  - 目標フィルター、回数制限、メール通知に対応
- ヒューズと保護：
  - 一部の高リスク操作をブロックまたは確認
  - 購入、スキップ、和了、退出などの保護ルールを設定
- デバッグと補助：
  - パケットパイプラインとモジュール順序を設定
  - 実行ログ、ゲーム状態、パケット詳細を表示
  - 独立した HUD ウィンドウに重要な情報を表示
- プラグイン：
  - プラグインマーケットまたはローカル ZIP からプラグインをインストール
  - プラグインの状態、パケット権限、自動更新を管理
  - プラグインでバックエンド処理モジュールとフロントエンドページを拡張

## 使用方法

以下の手順は Windows と Proxifier を例にしています。

### 1. Proxifier の設定

1. [Proxifier 公式サイト](https://www.proxifier.com/)から Proxifier をダウンロードしてインストールします。
2. `Profile -> Proxy Servers... -> Add...` を開きます。
3. ローカルプロキシを追加します。

   - `Address`：`127.0.0.1`
   - `Port`：`10999`
   - `Protocol`：`HTTPS`

4. Proxifier から既定のプロキシとして設定するか確認された場合は、`No` を選択します。
5. `Profile -> Proxification Rules... -> Add...` を開き、雀魂のプロセス用ルールを追加します。

   - クライアント版では通常 `Jantama_MahjongSoul.exe` を指定します
   - ブラウザ版では、実際に使用するブラウザプロセス（`chrome.exe` や `msedge.exe` など）を指定します
   - `Action` には、先ほど作成したローカルプロキシを指定します

Shanten Lens 自体をプロキシルールに追加しないでください。プロキシループが発生する可能性があります。

### 2. ルート証明書のインストール

MITM プロキシを初めて起動すると、アプリのデータディレクトリ内の `configs/ca/` に次のファイルが生成されます。

- `shanten-lens-ca.cer`
- `shanten-lens-ca.key`

アプリの設定から「設定ディレクトリを開く」を選択し、`ca` ディレクトリを開きます。`shanten-lens-ca.cer` をダブルクリックし、「ローカル コンピューター」の「信頼されたルート証明機関」に証明書をインストールします。

秘密鍵ファイル `shanten-lens-ca.key` はローカル端末内だけに保存し、共有やアップロードをしないでください。

### 3. 起動順序

推奨する起動順序：

1. Shanten Lens を起動
2. MITM プロキシが `127.0.0.1:10999` で待ち受けていることを確認
3. Proxifier を起動
4. 雀魂クライアントまたはブラウザを起動

注意：

- Proxifier が動作中で Shanten Lens が起動していない場合、ゲームの通信が存在しないローカルプロキシへ転送されるため、接続できなくなります。
- このツールを一部の場面でのみ利用する場合は、必要なときだけ Proxifier のルールを有効にできます。
- 設定画面で MITM ポートを変更した場合は、Proxifier 側のポートも同じ値に変更してください。

### 4. Cheat Engine との併用

Shanten Lens を Cheat Engine と併用する場合、一般的には次の用途があります。

- クライアントのアニメーションを高速化し、待ち時間を短縮する
- 自動化中にゲーム速度を `0` にする
- 高すぎる倍率による切断を避けるため、倍率を最大 `5` 以下にする

サードパーティ製ツールを使用する前に、リスクと適用される規約を自身で確認してください。

## 技術スタック

- フロントエンド：`React 18`、`TypeScript`、`Vite 5`
- デスクトップコンテナ：`Tauri 2`
- バックエンド：`Rust 2021`
- MITM プロキシ：`Hudsucker`
- プロトコル：`protobuf`、HTTPS、WebSocket
- フロントエンドとバックエンドの通信：Tauri プロセス内 IPC commands と events
- パッケージマネージャー：`pnpm 9`

Shanten Lens 3.0 は Rust バックエンドを Tauri プロセスに直接組み込んでいるため、Python バックエンド、sidecar、独立した UI WebSocket サービスは不要です。

## ディレクトリ構成

```text
.
├─ app/                 React フロントエンドと Tauri デスクトップアプリ
│  ├─ src/              ページ、コンポーネント、フロントエンドのビジネスロジック
│  ├─ public/           お守り、スタンプ、牌画像、Logo のリソース
│  └─ src-tauri/        Tauri Rust エントリーポイントと IPC コマンド
├─ backend/             組み込み Rust バックエンド、MITM、プラグイン、自動化ロジック
├─ proto/               ゲームプロトコル定義
├─ scripts/             Windows と macOS のパッケージ化スクリプト
├─ .github/workflows/   CI とリリースワークフロー
├─ Cargo.toml           Rust workspace
└─ README.md
```

## ビルドとパッケージ化

### 必要な環境

- Rust stable と Cargo
- Node.js 22
- pnpm 9
- 各プラットフォームで Tauri 2 に必要なシステム依存関係

フロントエンドの依存関係をインストールします。

```powershell
cd app
pnpm install
```

Rust の依存関係は、ビルド時に Cargo が自動的にダウンロードします。

### ローカル開発

Rust バックエンドは Tauri プロセスに組み込まれているため、個別の起動コマンドはありません。

```powershell
cd app
pnpm run tauri:dev
```

既定では、MITM プロキシは `127.0.0.1:10999` で待ち受けます。フロントエンドとバックエンドは Tauri IPC で直接通信します。

ブラウザ上のフロントエンド画面だけをデバッグする場合は、次のコマンドを実行します。

```powershell
cd app
pnpm run dev
```

ブラウザのみのモードでは、Tauri IPC に依存する完全なバックエンド機能は利用できません。

### Windows パッケージ

```powershell
scripts\build_all.bat
```

スクリプトはロックされたバージョンのフロントエンド依存関係をインストールし、組み込み Rust バックエンドを含む MSI インストーラーをビルドします。出力先：

```text
app/src-tauri/target/release/bundle/msi/
```

### macOS パッケージ

```bash
./scripts/build_macos.sh
```

スクリプトは `.app` と `.dmg` を生成します。macOS でのビルドには、Xcode Command Line Tools と Tauri に必要なシステム依存関係も必要です。出力先：

```text
app/src-tauri/target/release/bundle/macos/
app/src-tauri/target/release/bundle/dmg/
```

## プロジェクトの検証

Rust バックエンドのテスト：

```powershell
cargo test --manifest-path backend\Cargo.toml
```

TypeScript とフロントエンドの本番ビルド：

```powershell
cd app
pnpm run build
```

Tauri バックエンドのチェック：

```powershell
cargo check --manifest-path app\src-tauri\Cargo.toml
```

## プラグイン

関連プロジェクト：[プラグインマーケット](https://github.com/SpCoGov/shanten-lens-marketplace) · [プラグイン SDK](https://github.com/SpCoGov/shanten-lens-sdk)

アプリ内の「プラグイン」ページでは、プラグインマーケット、インストール済みプラグイン、プラグインソースを管理できます。

- プラグインマーケットからインストールするか、ルートに `plugin.json` を含む ZIP パッケージをインポートできます。
- プラグインを有効にする際はパケット権限の確認が必要です。内容を理解し、実際に必要な権限だけを付与してください。
- 更新の確認、自動更新、無効化、再スキャン、アンインストールに対応しています。
- プラグインは独立したバックエンドプロセス、フロントエンドページ、ローカライズリソース、パケット処理モジュールを提供できます。
- サードパーティ製プラグインは Shanten Lens のコアコードには含まれません。インストール前に配布元、権限、公開者を確認し、サードパーティ製コードを実行するリスクを自身で負ってください。

プラグインを手動で開発またはデバッグする場合は、プラグインページからプラグインディレクトリを開き、`plugin.json` を含むプラグインディレクトリを配置してから「再スキャン」を実行します。現在のプラグイン API バージョンは `1` です。

## よくある質問

### ゲームに突然接続できなくなったのはなぜですか？

最も多い原因は、Proxifier のルールが有効なまま Shanten Lens が起動していない、または両者の MITM ポート設定が一致していないことです。この場合、通信は利用できないプロキシポートへ転送されます。

### ブラウザ版でも証明書のインストールが必要ですか？

クライアント、ブラウザ、ログイン処理のいずれかが HTTPS 証明書を検証する場合は、Shanten Lens が生成したローカルルート証明書を信頼する必要があります。証明書が未インストール、または誤った証明書ストアにインストールされている場合、TLS 接続に失敗します。

### 牌山の表示が実際の牌山と一致しないのはなぜですか？

一部のお守りには、まだ完全には処理されていない特殊なケースがあります。アプリの「概要」ページにある「既知の問題」を確認し、そこに記載されたお守りは慎重に使用してください。

### 自動化の後、一時的にログインできないのはなぜですか？

自動操作の間隔が短すぎると、一時的な制限が発生する可能性があります。操作間隔は `400ms` 以上に設定することを推奨します。すでに制限されている場合は、自動化を停止し、回復を待つかネットワークを変更してください。

### 設定と証明書はどこに保存されますか？

OS が提供するアプリケーションデータディレクトリに保存されます。アプリの設定から「設定ディレクトリを開く」を選択できるため、完全なパスを手動で確認する必要はありません。証明書はその中の `ca/` にあります。

### このプロジェクトはサーバー上のデータを変更しますか？

アプリはゲーム通信をプロキシして処理し、一部の機能ではリクエストを送信または変更します。この README は現在のコード構成とローカル機能を説明するものであり、サードパーティサービス、アカウントの安全性、プラットフォームの規約について保証するものではありません。リスクを十分に理解した上で使用してください。

## ライセンス

プロジェクトのソースコードは [Apache License 2.0](./LICENSE) の下で公開されています。一部の牌画像およびゲーム関連素材は『雀魂』に由来し、非営利目的の学習と研究にのみ使用されています。これらの素材に関する権利は、プロジェクトのコードライセンスによって変更されるものではありません。
