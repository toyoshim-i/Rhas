# テストガイド

## 概要
rhas のテストは以下の 3 層で構成する。

| スイート | 場所 | 件数 | 目的 |
|---|---|---:|---|
| ユニットテスト | `src/**` | 多数 | モジュール単体の正確性 |
| ゴールデンテスト | `tests/golden_test.rs` | 23 | HAS060.X とのバイト一致 |
| 統合テスト | `tests/integration_test.rs` | 85 | 3パス全体の振る舞い検証 |
| エラーメッセージ比較 | `tests/error_message_test.rs` | 5 | 失敗時メッセージ互換の固定 |

## 実行コマンド
```bash
cargo test
cargo test --test golden_test
cargo test --test integration_test
cargo test --test error_message_test
./tests/compare_ms5_simple.sh
./tests/compare_ms6_extended.sh
```

## ゴールデンテスト
- ソース: `tests/asm/*.s`
- 参照出力: `tests/golden/*.o`
- 生成: `zsh tests/gen_golden.sh`
- `_opt.s` は `-c4` 前提 (`golden_test_opt!`)

現在の対象は以下を網羅:
- 68000 基本命令群
- EA モード
- 疑似命令（データ/シンボル/セクション/条件/マクロ）
- 式演算
- ROFST と最適化 (`addq_opt`)
- FPU コア (`fpu_core`)
- FMOVEM 制御レジスタ転送 (`fmovem_ctrl`)
- FMOVEM FPn 静的リスト (`fmovem_list`)
- FMOVEM FPn 動的リスト (`fmovem_dyn`)
- FPU 条件分岐/ループ (`fpu_branch`)
- `-c4` 拡張最適化主要ケース (`c4_core_opt`)

## 統合テストの主対象
- オブジェクト構造 (`$D000/$C0xx/$B2xx/$E001/$0000`)
- Pass1/2/3 の再評価と最適化（分岐縮小、`.equ/.set`、DeferredInsn）
- PRN 出力制御（`.list/.nlist/.sall/.lall/.width/.title/.subttl/.page`）
- MS6 進行分（`.offsym`, `.fpid`, SCD 疑似命令と SCD フッタ）

SCD まわりで現在固定している仕様:
- `-g` のみで `$B204` は出る
- `-g` では SCD 疑似命令（`.file/.def/.endef/...`）を無視（HAS互換）
- `-g` なしでは `.file` 検出後に SCD 疑似命令が有効（HAS互換）
- B204 のファイル名は入力ソース名を維持
- SCD フッタ `.file` 名は `-g` では入力ソース名、`.file` モードでは `.file` 指定名
- `.file` の exname は 14 文字超で使用
- SCD フッタの SCD エントリ列は `len` 依存の可変長

## 現在の結果（2026-03-01）
- `cargo test --test integration_test --quiet`: 85/85 pass
- `cargo test --test golden_test --quiet`: 23/23 pass
- `cargo test --test error_message_test --quiet`: 5/5 pass
- `./tests/compare_ms5_simple.sh`: 17/17 一致
- `./tests/compare_ms6_extended.sh`: 19/19 一致

## テスト追加ルール
1. 単体ロジックはユニットテストを優先
2. HAS 互換性を確認するものはゴールデンテスト
3. パス間相互作用やファイル出力仕様は統合テスト
4. 仕様変更時は docs とテスト名を同時更新

## 検証バックログ
残タスクは [verification_backlog.md](verification_backlog.md) で優先度付き管理に統一する。
