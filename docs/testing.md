# テストガイド

## 概要
rhas のテストは以下の 3 層で構成する。

| スイート | 場所 | 件数 | 目的 |
|---|---|---:|---|
| ユニットテスト | `src/**` | 多数 | モジュール単体の正確性 |
| ゴールデンテスト | `tests/golden_test.rs` | 25 | HAS060.X とのバイト一致 |
| 統合テスト | `tests/integration_test.rs` | 97 | 3パス全体の振る舞い検証 |
| エラーメッセージ比較 | `tests/error_message_test.rs` | 9 | 失敗時メッセージ互換の固定 |

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
- FMOVEM 制御レジスタ複合指定 (`fmovem_ctrl_list`)
- FMOVEM FPn 静的リスト (`fmovem_list`)
- FMOVEM FPn 動的リスト (`fmovem_dyn`)
- FPU 条件分岐/ループ (`fpu_branch`)
- FSINCOS (`fsincos`)
- `-c4` 拡張最適化主要ケース (`c4_core_opt`)

## 統合テストの主対象
- オブジェクト構造 (`$D000/$C0xx/$B2xx/$E001/$0000`)
- Pass1/2/3 の再評価と最適化（分岐縮小、`.equ/.set`、DeferredInsn）
- PRN 出力制御（`.list/.nlist/.sall/.lall/.width/.title/.subttl/.page`）
- MS6 進行分（`.offsym`, `.fpid`, `fsincos`, SCD 疑似命令と SCD フッタ）
- CPU 選択（ColdFire `.5200`/`.5300`/`.5400`、`.cpu` 数式パラメータ）
- 外部参照リロケーション（FBcc/FDBcc `.w`、Bcc.L/FBcc.L RPN リロケーション）

SCD まわりで現在固定している仕様:
- `-g` のみで `$B204` は出る
- `-g` では SCD 疑似命令（`.file/.def/.endef/...`）を無視（HAS互換）
- `-g` なしでは `.file` 検出後に SCD 疑似命令が有効（HAS互換）
- B204 のファイル名は入力ソース名を維持
- SCD フッタ `.file` 名は `-g` では入力ソース名、`.file` モードでは `.file` 指定名
- `.file` の exname は 14 文字超で使用
- SCD フッタの SCD エントリ列は `len` 依存の可変長

## 現在の結果（2026-03-01）
- `cargo test --test integration_test --quiet`: 97/97 pass
- `cargo test --test golden_test --quiet`: 25/25 pass
- `cargo test --test error_message_test --quiet`: 9/9 pass
- `./tests/compare_ms5_simple.sh`: 17/17 一致
- `./tests/compare_ms6_extended.sh`: 19/19 一致

## リグレッション利用チェック
クリーンアップ系変更では、以下を「互換性リグレッションセット」として扱う。

1. 常時実行可能（ローカルのみで完結）
- `cargo test --test integration_test --quiet`
- `cargo test --test golden_test --quiet`
- `cargo test --test error_message_test --quiet`

2. 外部ツール前提（HAS060.X + run68 が必要）
- `./tests/compare_ms5_simple.sh`
- `./tests/compare_ms6_extended.sh`

3. 判定ルール
- 1 の全通過を必須ゲートとする
- 2 は実行可能環境では必須、未実行時は未実行理由を記録する
- 互換性変更を含む場合は 1+2 を両方実行して結果をドキュメント反映する

## dead_code 調査・修正ルール（接続忘れ優先）
`dead_code` を減らす際は「単純削除」ではなく、まず接続忘れを疑う。

1. 判定
- `cargo test --lib --quiet` と `cargo test --bin rhas --quiet` を分離実行し、warning の発生ターゲットを特定する
- 原典（`external/has060xx/src`）に同等の処理経路がある場合は「未接続候補」とする

2. 修正順序
- 先に「期待挙動を落とすテスト」を追加して失敗を再現する
- 次に接続修正を実装する
- 最後に既存リグレッションセット（integration/golden/error_message/compare_ms5/compare_ms6）を全実行する

3. 判定基準
- 既存テストだけ通っても未接続候補はクローズしない
- 新規テストで「未接続経路を通ること」を確認できた場合のみクローズする

## テスト追加ルール
1. 単体ロジックはユニットテストを優先
2. HAS 互換性を確認するものはゴールデンテスト
3. パス間相互作用やファイル出力仕様は統合テスト
4. 仕様変更時は docs とテスト名を同時更新

## 直近追加の検知テスト（2026-03-01）
- ColdFire CPU 選択: `test_coldfire_cpu5200_directive` / `test_coldfire_cpu5300_directive` / `test_coldfire_cpu5400_directive`
- `.cpu` 数式パラメータ: `test_cpu_directive_68020` / `test_cpu_directive_5200`
- FBcc/FDBcc 外部参照: `test_fbcc_xref_generates_reloc` / `test_fdbcc_xref_generates_reloc`
- Bcc.L/FBcc.L RPN リロケーション: `test_bcc_long_xref_generates_rpn_reloc` / `test_fbcc_long_xref_generates_rpn_reloc`
- エラーメッセージ: `test_error_message_cpu_invalid_number`（`.cpu 99999` の不正値検出）
- Pass 遷移: `test_assemble_sets_final_pass_to_pass3`
- Warning 抑止: `test_warning_level_zero_suppresses_offsym_warning`

## 検証バックログ
残タスクは [verification_backlog.md](verification_backlog.md) で優先度付き管理に統一する。
