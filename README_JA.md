# Shanten Lens 3.0

Shanten Lens は『雀魂』の青雲之志モード向けデスクトップ補助ツールです。3.0 は Tauri 2 + React + TypeScript フロントエンドと組み込み Rust バックエンドで動作し、開発・ビルド・実行に Python は不要です。

## アーキテクチャ

- フロントエンドとバックエンドは Tauri 2 のプロセス内 commands/events で通信し、UI WebSocket ポートは使用しません。
- Rust バックエンドは Hudsucker による HTTPS/WebSocket MITM、protobuf デコード、ゲーム状態、パケットパイプライン、自動化を担当します。
- `backend/` が現在の Rust バックエンドです。

## ディレクトリ

```text
app/              React フロントエンドと Tauri アプリ
backend/          組み込み Rust バックエンド
proto/            ゲームプロトコル定義
scripts/          Windows/macOS パッケージスクリプト
```

## 開発実行

Rust、Node.js 22、pnpm 9 が必要です。

```powershell
cd app
pnpm install
pnpm run tauri:dev
```

MITM は既定で `127.0.0.1:10999` を使用します。初回起動時にアプリ設定ディレクトリの `ca/shanten-lens-ca.cer` が生成されます。Proxifier ではゲームプロセスをこの HTTPS プロキシへ転送し、証明書を信頼してください。

## パッケージ

Windows：

```powershell
scripts\build_all.bat
```

MSI は `app/src-tauri/target/release/bundle/msi/` に出力されます。

macOS：

```bash
./scripts/build_macos.sh
```

アプリと DMG は `app/src-tauri/target/release/bundle/macos/`、`app/src-tauri/target/release/bundle/dmg/` に出力されます。

両スクリプトは `pnpm install --frozen-lockfile` の後、Tauri で TypeScript、フロントエンド、Rust バックエンド、インストーラーを構築します。Python、sidecar、旧 IPC 互換層は含みません。

## 検証

```powershell
cargo test --manifest-path backend\Cargo.toml
cd app
pnpm run build
cargo check --manifest-path src-tauri\Cargo.toml
```
