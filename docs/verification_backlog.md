# 検証バックログ

## 目的
実装完了後の互換性検証を、優先度付きで継続管理する。

## 現在の前提（2026-03-01）
- `cargo test`: pass
- `golden_test`: 22/22 pass
- `integration_test`: 84/84 pass
- `error_message_test`: 5/5 pass
- MS6（FPU/SCD/残互換機能）実装済み

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
1. FPU 命令の未実装群の仕様確定とテスト化（着手中）
- 第1段: `fmovem` 制御レジスタ転送（`fpcr/fpsr/fpiar` ↔ メモリ）を実装・固定化済み
- 第1.5段: `fmovem` FPn 静的リスト（`fp0/fp1` 形式）を実装・固定化済み
- 第2段: `fbcc` / `fdbcc` 全条件バリアントを実装・固定化済み
- 残: `fmovem` の動的レジスタリスト系（`Dn` マスク）、`fsincos`
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
