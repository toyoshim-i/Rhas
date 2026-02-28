# テストガイド

## 概要
rhas のテストは以下の 3 層で構成する。

| スイート | 場所 | 件数 | 目的 |
|---|---|---:|---|
| ユニットテスト | `src/**` | 多数 | モジュール単体の正確性 |
| ゴールデンテスト | `tests/golden_test.rs` | 17 | HAS060.X とのバイト一致 |
| 統合テスト | `tests/integration_test.rs` | 60 | 3パス全体の振る舞い検証 |

## 実行コマンド
```bash
cargo test
cargo test --test golden_test
cargo test --test integration_test
./tests/compare_ms5_simple.sh
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

## 統合テストの主対象
- オブジェクト構造 (`$D000/$C0xx/$B2xx/$E001/$0000`)
- Pass1/2/3 の再評価と最適化（分岐縮小、`.equ/.set`、DeferredInsn）
- PRN 出力制御（`.list/.nlist/.sall/.lall/.width/.title/.subttl/.page`）
- MS6 進行分（`.offsym`, `.fpid`, SCD 疑似命令と SCD フッタ）

SCD まわりで現在固定している仕様:
- `-g` のみで `$B204` は出る
- SCD 疑似命令は `.file` 検出後のみ有効（HAS互換）
- B204 のファイル名は入力ソース名を維持
- `.file` は SCD 側ファイル名として保持
- `.file` の exname は 14 文字超で使用

## 現在の結果（2026-03-01）
- `cargo test --test integration_test --quiet`: 60/60 pass
- `cargo test --test golden_test --quiet`: 17/17 pass
- `./tests/compare_ms5_simple.sh`: 17/17 一致

## テスト追加ルール
1. 単体ロジックはユニットテストを優先
2. HAS 互換性を確認するものはゴールデンテスト
3. パス間相互作用やファイル出力仕様は統合テスト
4. 仕様変更時は docs とテスト名を同時更新

## 直近の追加（要約）
- `.file` と `B204` の責務分離をテスト固定
- SCD 疑似命令の有効化条件（`.file` 必須）をテスト固定
- `-g` のみ時の SCD デフォルト行エントリをテスト追加
- `.type` のロング化条件（0x20/0x30 のみ）をテスト固定
- `.scl 16` の section=-2 補正をフッタ出力で検証
- `.endef` の attrib 補完（function/tag/extern/static）をテスト固定
- `.file` exname 条件を 14 文字超に調整

## 残課題
- FPU 命令（68881/68882）のゴールデン/統合テスト追加
- `-c4` の未カバー最適化フラグ個別ゴールデン追加
- エラーメッセージ比較用の専用テストファイル整備
