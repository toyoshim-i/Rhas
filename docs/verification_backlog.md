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
1. 警告ゼロ化（優先度C）
- 対象: `cargo test` 実行時の `unused` / `dead_code` 系 warning
- 条件: 互換性テスト（`golden/integration/error_message/compare_ms5/compare_ms6`）を全通過した状態でのみマージ
- 進捗: 第1回として `unused import` / `unused var` / `unused mut` / `unused_doc_comment` の一部を除去済み（互換テスト全通過を確認）

2. 実装コメントの「未対応」整理（優先度C）
- 対象: 例として `pass3` の外部参照未対応コメントなど、将来仕様として残すか実装対象にするか未確定の箇所
- 条件: 方針（仕様として非対応維持 or 実装）を文書化してから着手

3. ドキュメント保守項目の整理（優先度C）
- 対象: 実装本体の進捗と直接関係しない資料（例: syscall メモ）の「残り」記述
- 条件: 実装タスクと混在しないよう、追跡先ドキュメントを明確化

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
