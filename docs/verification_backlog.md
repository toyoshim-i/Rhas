# 検証バックログ

## 目的
実装完了後の互換性検証を、優先度付きで継続管理する。

## 現在の前提（2026-03-02）
- `cargo test`: pass（警告ゼロ）
- `golden_test`: 47/63 pass（既存25 + 新規22 pass / 新規7 MISMATCH + 5 ERROR）
- `integration_test`: 97/97 pass
- `error_message_test`: 13/35 pass + 22 ignored（異常系仕様テスト）
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
- `cargo clippy --all-targets --all-features` 警告ゼロ達成（84件修正: doc comment 変換、range contains、collapsible match、redundant closure、unnecessary cast 等）

## ゴールデンテスト拡充（2026-03-01 調査）

原典 HAS060.X の走行結果を期待値として、未カバー領域のゴールデンテストを追加。
既存 25 件は全 pass を維持。新規 35 件の結果:

### Round 1: 68020+ 命令 / 式 / マクロ / 疑似命令

| テスト | カテゴリ | 結果 | 詳細 |
|---|---|---|---|
| `insn_link_unlk` | LINK/UNLK | **PASS** | — |
| `insn_moves` | MOVES (68010+) | **PASS** | — |
| `insn_chk` | CHK (68000/68020) | MISMATCH | 90 vs 92 bytes |
| `insn_020` | EXTB/PACK/UNPK/RTD/LINK.L/MOVEC | MISMATCH | 134 vs 118 bytes（rhas が大きい） |
| `expr_bitwise` | ビット演算式 (~, &, \|, ^, <<, >>) | MISMATCH | 132 vs 156 bytes（rhas が小さい） |
| `pseudo_offset` | .offset 疑似命令 | MISMATCH | 104 vs 100 bytes |
| `ea_020` | 68020 EA (indexed+scale) | ERROR | PC相対indexed構文パース失敗 |
| `insn_bfld` | ビットフィールド (BFTST等) | **PASS** | `{offset:width}` 構文対応済み |
| `insn_cas` | CAS2 (68020) | **PASS** | CAS2 命令対応済み |
| `insn_muldiv_l` | DIVSL/DIVUL (68020) | **PASS** | DIVSL/DIVUL + MULS.L/MULU.L/DIVS.L/DIVU.L 長形式対応済み |
| `pseudo_macro_adv` | ネストマクロ/rept/irp | **PASS** | `\` エスケープ + `-u` フラグ対応済み |

### Round 2: 最適化レベル / SCD / MOVEM / MOVEP / ローカルラベル

| テスト | カテゴリ | 結果 | 詳細 |
|---|---|---|---|
| `opt_branch` | 分岐最適化 (デフォルト) | **PASS** | — |
| `insn_movep` | MOVEP | **PASS** | — |
| `pseudo_local` | 数値ローカルラベル | **PASS** | — |
| `pseudo_comm` | .comm/.xdef/.xref | **PASS** | — |
| `opt_c0` | -c0 最適化無効 | MISMATCH | 112 vs 114 bytes |
| `opt_c2` | -c2 v2互換モード | MISMATCH | 104 vs 94 bytes（rhas が大きい） |
| `scd_g` | -g SCD デバッグ出力 | MISMATCH | 406 vs 412 bytes |
| `scd_file` | .file SCD モード | MISMATCH | 274 vs 280 bytes |
| `insn_movem` | MOVEM バリエーション | ERROR | 単一レジスタ MOVEM パース失敗 |

### Round 3: BCD / DEC・INC / TRAPcc / キャッシュ / メモリ間接 / 相対セクション

| テスト | カテゴリ | 結果 | 詳細 |
|---|---|---|---|
| `insn_bcd` | ABCD/SBCD | **PASS** | — |
| `insn_dec_inc` | DEC/INC (SUBQ/ADDQ #1) | **PASS** | — |
| `insn_trapcc` | TRAPcc 条件トラップ (68020+) | **PASS** | — |
| `pseudo_data_str` | 文字列リテラル (.dc.b) | **PASS** | — |
| `insn_chk2_cmp2` | CHK2/CMP2 (68020+) | MISMATCH | 152 vs 176 bytes |
| `pseudo_rsect` | 相対セクション (.rdata/.rbss/.rstack) | MISMATCH | 172 vs 212 bytes |
| `insn_cache` | CINV/CPUSH キャッシュ命令 (68040+) | ERROR | CINV/CPUSH 構文パース失敗 |
| `insn_move16` | MOVE16 (68040+) | ERROR | 非ポストインクリメント形式未対応 |
| `ea_memory_indirect` | メモリ間接EA (68020+) | ERROR | メモリ間接アドレッシング未対応 |

### Round 4: MOVEQ最適化 / JMP-JSR / DBcc / 絶対アドレッシング / 条件分岐 / rept

| テスト | カテゴリ | 結果 | 詳細 |
|---|---|---|---|
| `opt_moveq` | MOVEQ 境界値最適化 | MISMATCH | 132 vs 112 bytes（rhas が大きい） |
| `opt_jmp_jsr` | JMP/JSR 最適化 (-c4) | **PASS** | — |
| `insn_dbcc` | DBcc 全条件コード | **PASS** | — |
| `ea_absshort` | 絶対ショート/ロング/PC相対 | **PASS** | — |
| `pseudo_cond_edge` | .ifdef/.elseif ネスト | **PASS** | — |
| `pseudo_rept_edge` | .rept 0/ネスト/.irp/.irpc | **PASS** | — |

### 集計

| 結果 | Round 1 | Round 2 | Round 3 | Round 4 | 合計 |
|---|---|---|---|---|---|
| PASS | 6 | 4 | 4 | 5 | **19** |
| MISMATCH | 4 | 4 | 2 | 1 | **11** |
| ERROR | 1 | 1 | 3 | 0 | **5** |
| **合計** | 11 | 9 | 9 | 6 | **35** |

### 最適化比較テスト

| テスト | 内容 | 結果 |
|---|---|---|
| `opt_compare` | デフォルト最適化で出力 | **PASS** |
| `opt_compare_c4` | -c4 最適化で同じソースを出力 | **PASS** |
| `opt_compare_default_vs_c4_differ` | 両者のサイズ差を検証 | **PASS** |

デフォルト 122 bytes → -c4 で 108 bytes（14 bytes 削減）。
CLR→MOVEQ, MOVEA.L→W, CMPI→TST, LEA→ADDQ, ASL→ADD, SUBI/ADDI #0 除去を確認。

## 異常系テスト拡充（error_message_test）

HAS060.X と rhas の異常系動作を比較。26 件追加（うち 22 件は `#[ignore]` で仕様記録）。

### 結果

| テスト | カテゴリ | 結果 | 不具合の種類 |
|---|---|---|---|
| `test_error_extb_on_68000` | CPU ゲート | **PASS** | — |
| `test_error_pack_on_68000` | CPU ゲート | **PASS** | — |
| `test_ok_extb_on_68020` | CPU ゲート正常系 | **PASS** | — |
| `test_ok_bcc_long_on_68020` | CPU ゲート正常系 | **PASS** | — |
| `test_error_bcc_long_on_68000` | CPU ゲート | IGNORED | エラー検出欠落 |
| `test_error_chk_long_on_68000` | CPU ゲート | IGNORED | エラー検出欠落 |
| `test_error_moveq_overflow` | 即値範囲 | IGNORED | エラー検出欠落 |
| `test_error_moveq_negative_overflow` | 即値範囲 | IGNORED | 汎用メッセージ |
| `test_error_addq_overflow` | 即値範囲 | IGNORED | 汎用メッセージ |
| `test_error_addq_zero` | 即値範囲 | IGNORED | 汎用メッセージ |
| `test_error_bra_short_out_of_range` | 分岐範囲 | IGNORED | エラー検出欠落 |
| `test_error_shift_count_overflow` | シフト範囲 | IGNORED | 汎用メッセージ |
| `test_error_shift_count_zero` | シフト範囲 | IGNORED | 汎用メッセージ |
| `test_error_div_zero` | ゼロ除算 | IGNORED | エラー検出欠落 |
| `test_error_address_register_byte` | サイズ制約 | IGNORED | エラー検出欠落 |
| `test_error_memory_shift_non_word` | サイズ制約 | IGNORED | 汎用メッセージ |
| `test_error_memory_bit_non_byte` | サイズ制約 | IGNORED | エラー検出欠落 |
| `test_error_register_bit_non_long` | サイズ制約 | IGNORED | エラー検出欠落 |
| `test_error_move_to_ccr_non_word` | サイズ制約 | IGNORED | エラー検出欠落 |
| `test_error_move_to_sr_non_word` | サイズ制約 | IGNORED | エラー検出欠落 |
| `test_error_undefined_symbol` | シンボル | IGNORED | エラー検出欠落 |
| `test_error_symbol_redefinition` | シンボル | IGNORED | エラー検出欠落 |
| `test_error_unclosed_string` | 構文 | IGNORED | エラー検出欠落 |
| `test_error_endm_without_macro` | マクロ | IGNORED | エラー検出欠落 |
| `test_error_else_without_if` | 条件分岐 | IGNORED | エラー検出欠落 |
| `test_error_endif_without_if` | 条件分岐 | IGNORED | エラー検出欠落 |

### 不具合分類

| 分類 | 件数 | 内容 |
|---|---|---|
| エラー検出欠落 | 16 | rhas がエラーとすべき入力を成功させてしまう |
| 汎用メッセージ | 6 | エラーは出るが具体的なメッセージではなく汎用の「記述が間違っています」 |

### 修正優先度

**高（ERROR — 機能欠落）:**
1. ~~ネストマクロ呼び出し~~ — 対応済み（`\` エスケープ + `-u` フラグ）
2. ~~ビットフィールド命令~~ — 対応済み（`{offset:width}` 構文 + レジスタオフセットエンコード修正）
3. ~~CAS2 / DIVSL・DIVUL~~ — 対応済み（CAS2 + MULS.L/MULU.L/DIVS.L/DIVU.L 長形式 + DIVSL/DIVUL）
4. 68020 EA indexed+scale 構文
5. 単一レジスタ MOVEM パース
6. メモリ間接アドレッシング（68020+）
7. CINV/CPUSH キャッシュ命令（68040+）
8. MOVE16 非ポストインクリメント形式（68040+）

**高（エラー検出欠落 — 正常系が生成するが本来エラー）:**
9. 未定義シンボル検出
10. シンボル再定義検出
11. Bcc.L の 68000 CPU ゲート
12. CHK.L の 68000 CPU ゲート
13. MOVEQ 即値範囲チェック
14. bra.s 分岐範囲チェック
15. ゼロ除算検出
16. MOVE to CCR/SR サイズ制約
17. An バイトアクセス拒否
18. メモリビット操作サイズ制約
19. 閉じていない文字列リテラル
20. 孤立 .endm/.else/.endif 検出

**中（MISMATCH — エンコード/出力差異）:**
21. -c0 / -c2 最適化レベル切替の挙動差異
22. SCD デバッグ出力 (-g / .file) のサイズ差異
23. CHK / EXTB・PACK・RTD・LINK.L・MOVEC エンコード差異
24. ビット演算式の評価差異
25. .offset セクションサイズ差異
26. CHK2/CMP2 エンコード差異
27. 相対セクション出力差異
28. MOVEQ 境界値最適化の差異（move.l #imm,Dn の MOVEQ 変換範囲）

**低（汎用メッセージ — エラーは出るがメッセージが不正確）:**
29. ADDQ/SUBQ 範囲外メッセージ
30. MOVEQ 負値範囲外メッセージ
31. シフトカウント範囲外メッセージ
32. メモリシフト/ローテートサイズメッセージ

## 残タスク（クリーンアップ系）

### 1. 実装コメントの「未対応」整理（優先度C）
- 対象: ソース中の「未対応」「TODO」等のコメント
- 条件: 方針（仕様として非対応維持 or 実装）を文書化してから着手
- 備考: Bcc.L/FBcc.L 外部参照は実装済みのため、関連コメントは更新対象

### 2. ドキュメント保守項目の整理（優先度C）— 対応済み
- 「Rustへの移植方針」セクション: 3ファイルから削除済み（`338e12f`）

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
