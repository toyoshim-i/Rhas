# rhas
[![CI](https://github.com/toyoshim-i/Rhas/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/toyoshim-i/Rhas/actions/workflows/ci.yml)
[![Compat Manual Status](https://img.shields.io/endpoint?url=https%3A%2F%2Fraw.githubusercontent.com%2Ftoyoshim-i%2Faction-stores%2Fmain%2Fbadges%2Frhas%2Fcompat-manual.json)](https://github.com/toyoshim-i/Rhas/actions/workflows/ci.yml?query=event%3Aworkflow_dispatch)
[![Last Commit (main)](https://img.shields.io/github/last-commit/toyoshim-i/Rhas/main?logo=github)](https://github.com/toyoshim-i/Rhas/commits/main)

X68000 用 M68000 アセンブラ **HAS060X.X** を Rust に移植したクロスアセンブラです。

> **実験的 AI 実装**: このプロジェクトのコードは全面的に AI が生成しました。
> 人間が仕様・方針を決定し、コードの生成・修正・テストを AI に委任するという
> 実験的な開発手法の検証を兼ねています。

---

## 概要

| 項目 | 内容 |
|---|---|
| ベース | HAS060X.X v1.2.5（HAS060.X v3.09+91 の改造版） |
| ターゲット CPU | M68000 / 68010 / 68020 / 68030 / 68040 / 68060 / ColdFire 5200/5300/5400 |
| 出力形式 | HLK オブジェクトファイル（Human68k リンカ互換） |
| 実装言語 | Rust（エディション 2021） |
| プラットフォーム | Linux / macOS / Windows（クロスプラットフォームネイティブ動作） |

オリジナルの HAS060X.X は Human68k（X68000 の OS）上でのみ動作する M68000 バイナリです。
rhas はその動作を Rust で完全再現し、現代の PC 上でクロス開発ができることを目的としています。

---

## ビルド

Rust ツールチェーン（1.70 以降）が必要です。

```bash
cargo build --release
# バイナリ: target/release/rhas
```

---

## 使い方

オリジナルの HAS060X.X と互換のオプションを受け付けます。

```bash
# 基本的なアセンブル
rhas source.s

# 出力ファイル名指定
rhas -o output.o source.s

# 未定義シンボルを外部参照として扱う（Human68k での標準的な使い方）
rhas -u source.s

# -c4 拡張最適化を有効にする
rhas -c4 -u source.s

# ヘルプ
rhas -h
```

### 主なオプション

| オプション | 内容 |
|---|---|
| `-u` | 未定義シンボルを外部参照（`.xref`）として扱う |
| `-c4` | 拡張最適化を全て有効にする（`ADD #1-8` → `ADDQ` 等） |
| `-p` | PRN リストファイルを生成する |
| `-o <file>` | 出力ファイル名を指定する |
| `-I <dir>` | インクルードパスを追加する |
| `-8` | シンボル名を最大 8 文字に制限する |
| `-w0` | 警告レベルを 0（全警告抑制）に設定する |
| `-1` | 68010 以降の命令を有効にする |

---



## テスト

```bash
# 全テスト実行（ユニット + ゴールデン + 統合）
cargo test

# ゴールデンテスト（HAS060.X 出力とのバイト比較）のみ
cargo test --test golden_test

# 実ソース比較（MS5 / MS6 拡張）
./tests/compare_ms5_simple.sh
./tests/compare_ms6_extended.sh

# ゴールデンファイルの再生成（run68 + HAS060.X が必要）
zsh tests/gen_golden.sh
```

詳細は [docs/testing.md](docs/testing.md) を参照してください。

現在の主なテスト結果:
- `clippy`: 警告ゼロ
- `cargo test (unit / integration tests)`: 231/231 pass
- `golden_test`: 63/63 pass（100% 互換）
- `error_message_test`: 36/36 pass（100% 互換）
- `compare_ms5_simple.sh`: 17/17 一致
- `compare_ms6_extended.sh`: 19/19 一致

---

## 💻 VS Code 連携 (LSP & 構文ハイライト)

本プロジェクトは、VS Code 上での開発を支援する拡張機能（シンタックスハイライト、エラーチェック、ホバー表示、定義ジャンプ、コード補完、フォーマッタなど）を同梱しています。

機能の詳細、設定方法、および導入手順については、以下の拡張機能ドキュメントを参照してください：
- 📄 **[VS Code 拡張機能ドキュメント (vscode/README.md)](vscode/README.md)**

---

## ライセンス

このプロジェクトは HAS060X.X（HAS060.X / HAS.X の改造版）の移植です。
著作権については以下の通りです。

- **HAS.X v3.09 の基本部分**: 著作権は原作者 中村 祐一 氏にあります。
- **HAS060.X の改造部分**: 著作権は改造者 M.Kamada 氏にあります。
- **HAS060X.X の改造部分**: 著作権は改造者 TcbnErik 氏にあります。
  - リポジトリ: https://github.com/kg68k/has060xx

配布規定については HAS060.X に準じるものとします。

---

## 関連ドキュメント

| ドキュメント | 内容 |
|---|---|
| [docs/has_architecture.md](docs/has_architecture.md) | HAS の全体構造・3パス方式 |
| [docs/original_has_optimizations.md](docs/original_has_optimizations.md) | オリジナルHASの最適化仕様とフラグ対応一覧 |
| [docs/hlk_object_format.md](docs/hlk_object_format.md) | HLK オブジェクトファイルフォーマット仕様 |
| [docs/m68000_addressing.md](docs/m68000_addressing.md) | M68000 実効アドレスモード仕様 |
| [docs/implementation_progress.md](docs/implementation_progress.md) | 実装フェーズ別の詳細進捗 |
| [docs/testing.md](docs/testing.md) | テスト戦略・実行手順・現行カバレッジ |
| [docs/verification_backlog.md](docs/verification_backlog.md) | 積み残し検証項目（優先度付き） |
