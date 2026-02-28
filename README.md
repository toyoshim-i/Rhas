# rhas

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

## 実装状況

### マイルストーン

| MS | 達成条件 | 状態 |
|---|---|---|
| MS1 | `move.b d0,d1` → 正しいバイト列・最小限の HLK 出力 | ✅ 達成 |
| MS2 | 68000 全整数命令エンコード + ラベル・外部参照解決 | ✅ 達成 |
| MS3 | 疑似命令・最適化込みで `HANOI.S`（付属サンプル）が通る | ✅ 達成 |
| MS4 | マクロ処理込みで `K_MACRO.MAC`（構造化マクロライブラリ）が通る | ✅ 達成 |
| MS5 | 実 X68000 プログラムのビルドがオリジナルと完全一致 | ✅ 達成（17/17 ファイル一致） |
| MS6 | FPU（68881/68882）/ SCD デバッグ情報 / 残互換機能（`.fpid`/`.offsym` など） | ✅ 達成（比較 19/19 一致） |

### 機能実装状況

| 機能 | 状態 | 備考 |
|---|---|---|
| CLI（HAS060X 互換オプション） | ✅ 完了 | 全スイッチ対応 |
| 式評価（RPN） | ✅ 完了 | 全演算子・`.defined.` 等 |
| EA モード解析・エンコード | ✅ 完了 | 68000 基本 12 モード |
| 68000 全整数命令 | ✅ 完了 | |
| 68010–68060 拡張命令 | ✅ 完了 | |
| ColdFire 5200/5300/5400 命令 | ✅ 完了 | |
| 疑似命令（`.dc` `.ds` `.equ` `.if` 等） | ✅ 完了 | |
| マクロ（`.macro` `.rept` `.irp` `.irpc`） | ✅ 完了 | |
| 3 パス処理（分岐最適化含む） | ✅ 完了 | |
| HLK オブジェクトファイル出力 | ✅ 完了 | |
| PRN リストファイル出力 | ✅ 完了 | |
| `-c4` 拡張最適化（`ADD #1-8` → `ADDQ` 等） | ✅ 実装済み | ゴールデン `addq_opt` / MS5比較で検証済み |
| FPU 命令（68881/68882） | ✅ 実装済み | `fnop` / `fsave` / `frestore` / `fmove` / `fmovecr` / `fadd` / `fsub` / `fmul` / `fdiv` / `fcmp` / `ftst` / `fbcc` / `fdbcc` / `fsincos` / `fmovem`（制御レジスタ単体・複合、FPn静的/動的リスト、`.fpid` 反映含む） |
| SCD デバッグ情報（`-g` / `.file`） | ✅ 実装済み | HAS 互換の2モードを実装。`-g` では SCD 疑似命令を無視し、入力ソース名で SCD フッタ/B204 を出力。`.file` モード（`-g` なし）では SCD 疑似命令を有効化し、`.file` 指定名で SCD フッタを出力。可変長 SCD エントリ（`len` 依存）と `.file` 長名/`SCDFILENUM` も対応済み |

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

2026-03-01 時点の主な結果:
- `golden_test`: 25/25 pass
- `integration_test`: 87/87 pass
- `error_message_test`: 6/6 pass
- `compare_ms5_simple.sh`: 17/17 一致
- `compare_ms6_extended.sh`: 19/19 一致

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
