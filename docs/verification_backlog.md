# 検証バックログ

## 目的
実装完了後の互換性検証を、優先度付きで継続管理する。

## 現在の前提（2026-03-01）
- `cargo test`: pass
- `golden_test`: 25/25 pass
- `integration_test`: 87/87 pass
- `error_message_test`: 6/6 pass
- MS6（FPU/SCD/残互換機能）実装済み
- 優先度A/Bで追跡していた互換性検証項目は完了

## 残タスク（クリーンアップ系）
1. 警告ゼロ化（優先度C）✅
- 対象: `cargo test` 実行時の `unused` / `dead_code` 系 warning
- 条件: 互換性テスト（`golden/integration/error_message/compare_ms5/compare_ms6`）を全通過した状態でのみマージ
- 進捗: 第1回として `unused import` / `unused var` / `unused mut` / `unused_doc_comment` の一部を除去済み（互換テスト全通過を確認）
- 進捗: 第2回として `pass2` 分岐最適化ロジックを式化し、未使用再エクスポート警告を整理。互換テスト（87/25/6, 17/19比較）を再通過
- **完了**: `cargo check` で警告ゼロ達成（分類B除去+分類Cテストゲート+分類A抑制）

2. 実装コメントの「未対応」整理（優先度C）
- 対象: 例として `pass3` の外部参照未対応コメントなど、将来仕様として残すか実装対象にするか未確定の箇所
- 条件: 方針（仕様として非対応維持 or 実装）を文書化してから着手

3. ドキュメント保守項目の整理（優先度C）
- 対象: 実装本体の進捗と直接関係しない資料（例: syscall メモ）の「残り」記述
- 条件: 実装タスクと混在しないよう、追跡先ドキュメントを明確化

## dead_code 由来調査（2026-03-01）
- 調査方法:
- `cargo test --lib --quiet` と `cargo test --bin rhas --quiet` を分離して warning を比較
- `rg` で Rust 側参照有無を確認
- `external/has060xx/src`（`opname.s`/`error.s`/`work.s` 等）を照合

### 分類A: 原典由来の仕様テーブル/ワーク項目を先行移植した結果
- 例: `error.rs` の `ErrorCode`/`WarnCode` 群、`context.rs` の `AsmPass::Pass2/Pass3` と一部ワーク項目、`symbol/types.rs` の一部定数/variant
- 判定根拠: 原典に同等テーブル/ワーク領域が存在（`error.s` の `warntbl`、`work.s` の `ASMPASS` など）
- 補足: 現状の Rust 実行経路で未参照なものがあり、`bin` 単体ビルドで dead_code warning 化

### 分類B: 移植過程で残った補助/試作コード（欠落機能の直接証拠は薄い）
- `src/instructions/mod.rs`: `encode_not`（`NegNot` 共通化後のラッパ残り）
- `src/pass/pass1.rs`: `set_location`（未呼び出し）
- `src/pass/pass1.rs`: `InsnHandlerAlias`（ダミーtrait、実利用なし）
- `src/pass/pass3.rs`: `token_as_const`（現ロジック未使用）
- `src/instructions/mod.rs`(test): `encode_ok`（テスト補助だが未使用）

### 分類C: テスト専用利用で本体から未参照
- `src/pass/prn.rs` の `DEFAULT_PRN_*` はテスト内でのみ利用
- 判定根拠: `#[cfg(test)]` 配下からのみ参照

### 要確認（移植未接続の可能性がある箇所）
1. `error.rs` の `print_error`/`print_warning` 経路
- 原典は `error.s` の `printerr`/`warntbl` を中心に運用
- 現状は pass 側で直接文言出力する箇所が多く、テーブル定義が未接続

2. `AsmPass` の実遷移
- 原典は `ASMPASS=1/2/3` を処理中に切り替える
- 現状は enum 定義はあるが、実行中の明示更新経路が限定的

### 結論
- dead_code の大半は「原典に要素はあるが Rust 実行経路に未接続」または「移植中の補助コード残り」。
- 直ちに「機能欠落」と断定できるものは限定的だが、`error.rs` 経路と `AsmPass` 遷移は互換性観点で優先確認対象。
- 進捗: 第3回として「失敗再現テスト先行」で `AsmPass` 遷移と warning レベル反映を接続し、回帰セット（87/25/7, 17/19比較）を再通過。
- 進捗: 第4回として pass1 の全 `error()`/`warn()` インライン呼び出しを `error_code()`/`warn_code()` に置換し、`error.rs` テーブル経由に統一。回帰セット（88/25/8）を再通過。
- 進捗: 第5回として分類B dead_code（`encode_not`/`set_location`/`InsnHandlerAlias`/`token_as_const`/`Pass1Error`/`Pass3Error`）を除去、分類C をテストゲート化、分類A に `#![allow(dead_code)]` 付与。**警告ゼロ達成**。回帰セット（88/25/8）を再通過。

## 優先度A'（テスト欠落補完: 接続忘れ検知）
1. `error.rs` 出力経路の到達テスト追加
- 目的: `print_error`/`print_warning` 相当経路が実際に使用されることを検証し、未接続を検知可能にする
- 完了条件: 到達を確認するテストを追加し、接続修正の前後で挙動差を検出できる
- 状態: pass1 のエラー経路は対応済み。`error()` インライン呼び出しを全廃し `error_code()` / `warn_code()` 経由に統一。ErrorCode 12種（BadOpe/Forced/NoSymMacro/NoSymPseudo/Redef/RedefOffsym/IlSymValue/IlOpr/IlOprTooMany/IlValue/Expr/OffsymAlign）および WarnCode 1種（REDEF_OFFSYM）が接続済み。error_message_test 8件通過。

2. Pass 遷移（1→2→3）可視化テスト追加
- 目的: `AsmPass` が原典想定どおり遷移するかを検証し、状態未更新を検知可能にする
- 状態: 対応済み
- メモ: `tests/integration_test.rs` に `test_assemble_sets_final_pass_to_pass3` を追加し、`assemble()` 側で `Pass1→Pass2→Pass3` を明示更新

3. 未接続候補ごとの「再現→修正→回帰」テンプレ運用
- 目的: warning削減のたびに機能死を見逃さない
- 完了条件: 以後の dead_code 対応は必ず「失敗テスト先行」で実施し、結果を本書に追記

## 優先度A（次に必ず実施）
1. FPU ゴールデン比較を追加 ✅
- 対象: `fmove/fadd/fsub/fmul/fdiv/fcmp/ftst/fmovecr/fnop/fsave/frestore`
- 条件: `.fpid` 0..7、サイズ有無（デフォルト含む）、EA バリエーション（Dn/An/(An)/即値）
- 目的: HAS060.X とのバイト一致を統合テスト以外でも固定

2. `-c4` 最適化の未カバーケース追加 ✅
- 対象: 既存ゴールデンで未網羅の最適化フラグ組み合わせ
- 目的: 最適化回帰の早期検知

3. エラーメッセージ比較テスト整備 ✅
- 対象: サイズ不正、オペランド不正、式不正、SCD/FPU 境界
- 目的: 「失敗時の互換性」を固定

## 優先度B（継続的に追加）
1. FPU 命令の仕様確定とテスト化（完了）
- 第1段: `fmovem` 制御レジスタ転送（`fpcr/fpsr/fpiar` ↔ メモリ）を実装・固定化済み
- 第1.2段: `fmovem` 制御レジスタ複合指定（`fpcr/fpsr`）を実装・固定化済み
- 第1.5段: `fmovem` FPn 静的リスト（`fp0/fp1` 形式）を実装・固定化済み
- 第1.8段: `fmovem` FPn 動的リスト（`Dn` マスク）を実装・固定化済み
- 第2段: `fbcc` / `fdbcc` 全条件バリアントを実装・固定化済み
- 第2.5段: `fsincos`（`FPn`/EA ソース + `FPc:FPs` 宛先）を実装・固定化済み
- 第3段: `fmovem` のサイズ境界（禁止形式）のエラーメッセージ比較を実装・固定化済み
- 方針: 先に HAS060.X 側の最小ケースを確定し、その後実装

2. SCD の追加境界ケース ✅
- 長名・exname 境界（14/15）を追加
- `.tag` 連鎖（未解決指定の後に解決可能タグを再指定）を追加
- `next` チェイン異常系（孤立 `.eb/.ef`）を追加

3. 実プログラム比較セットの拡張 ✅
- `tests/compare_ms6_extended.sh` を追加
- MS5 の 17 ケース + `ms6_fpu_real` + `ms6_scd_real` の計 19 ケース比較を追加
- 実行結果: `RESULT ok=19 diff=0`

## 進め方
1. 先に「再現入力（asm）+ HAS期待値」を作る
2. `integration_test` で挙動固定
3. `golden_test` でバイト一致を固定
4. 差分が出た場合は `docs/testing.md` に追記
