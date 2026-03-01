# 検証バックログ

## 目的
実装完了後の互換性検証を、優先度付きで継続管理する。

## 現在の前提（2026-03-01）
- `cargo test`: pass（警告ゼロ）
- `golden_test`: 25/25 pass
- `integration_test`: 97/97 pass
- `error_message_test`: 9/9 pass
- `compare_ms5_simple.sh`: 17/17 一致
- `compare_ms6_extended.sh`: 19/19 一致
- MS6（FPU/SCD/残互換機能）実装済み
- 互換ギャップ（ColdFire CPU / `.cpu` / FBcc・FDBcc 外部参照 / Bcc.L・FBcc.L RPN リロケーション）全解消
- `error.rs` テーブル経由エラー出力: pass1 完了（ErrorCode 12種 + WarnCode 1種）
- dead_code 警告ゼロ達成（分類B除去 + 分類Cテストゲート + 分類A抑制）

## 完了済み項目（概要）

### 互換ギャップ（全解消）
| 項目 | コミット |
|---|---|
| ColdFire CPU 選択（`.5200`/`.5300`/`.5400`） | `c7f715a` |
| `.cpu` 数式パラメータ処理 | `1ca0181` |
| FBcc/FDBcc 外部参照（`.w` リロケーション） | `98fb1b3` |
| Bcc.L/FBcc.L RPN リロケーション | `1d75cb2` |

### テスト欠落補完（優先度A'）
- `error.rs` 出力経路の到達テスト: 対応済み（error_message_test 9件）
- Pass 遷移（1→2→3）可視化テスト: 対応済み
- 「再現→修正→回帰」テンプレ運用: 確立済み

### 検証テスト拡充（優先度A/B）
- FPU ゴールデン比較: 対応済み（`fpu_core`/`fmovem_*`/`fpu_branch`/`fsincos`）
- `-c4` 最適化未カバーケース: 対応済み（`c4_core_opt`）
- エラーメッセージ比較: 対応済み（9件）
- SCD 追加境界ケース: 対応済み
- MS6 実プログラム比較: 対応済み（19件）

### 警告ゼロ化
- `cargo check` 警告ゼロ達成

## 残タスク（クリーンアップ系）

### 1. 実装コメントの「未対応」整理（優先度C）
- 対象: ソース中の「未対応」「TODO」等のコメント
- 条件: 方針（仕様として非対応維持 or 実装）を文書化してから着手
- 備考: Bcc.L/FBcc.L 外部参照は実装済みのため、関連コメントは更新対象

### 2. ドキュメント保守項目の整理（優先度C）
- 対象: 参照資料（`has_architecture.md`/`hlk_object_format.md`/`m68000_addressing.md`）の「Rustへの移植方針」セクション
- 状態: 初期の移植計画時に書かれた案が実際のコード構造と乖離している
- 条件: 実態に合わせて更新するか、参照用資料として役割を明確化して移植方針セクションを削除

## dead_code 調査の記録

調査方法は `cargo test --lib` / `cargo test --bin rhas` の分離実行 + 原典照合。

| 分類 | 内容 | 対応 |
|---|---|---|
| A: 原典由来テーブル | `ErrorCode`/`WarnCode` 群、`AsmPass` 等 | `#![allow(dead_code)]` 抑制 |
| B: 移植過程の試作コード | `encode_not`/`set_location`/`InsnHandlerAlias` 等 | 除去済み |
| C: テスト専用 | `DEFAULT_PRN_*` | `#[cfg(test)]` ゲート化 |

要確認だった `error.rs` 出力経路と `AsmPass` 遷移はいずれも接続・テスト済み。

## 進め方
1. 先に「再現入力（asm）+ HAS期待値」を作る
2. `integration_test` で挙動固定
3. `golden_test` でバイト一致を固定
4. 差分が出た場合は `docs/testing.md` に追記
