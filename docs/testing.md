# テスト戦略とテストケース一覧

## 概要

rhas のテストは 3 層で構成される。

| スイート | 場所 | 件数 | 目的 |
|---|---|---|---|
| ユニットテスト | `src/**` 内 `#[cfg(test)]` | 180件 | 個別モジュールの正確性 |
| ゴールデンテスト | `tests/golden_test.rs` | 17件 | HAS060.X との出力一致検証 |
| 統合テスト | `tests/integration_test.rs` | 14件 | 3パス全体のエンドツーエンド |

```
cargo test          # 全スイート（211件）を実行
cargo test --test golden_test        # ゴールデンテストのみ
cargo test --test integration_test  # 統合テストのみ
```

---

## 1. ユニットテスト（180件）

各モジュールの `#[cfg(test)]` ブロックに配置。

### src/expr/ — RPN・式評価（~70件）

| テスト群 | 内容 |
|---|---|
| `parse_expr` | 10進・16進・8進・2進リテラル |
| 演算子テスト | `+` `-` `*` `/` `.mod.` `>>` `<<` `=` `<>` `&` `^` `\|` |
| 単項演算子 | `.not.` `.high.` `.low.` `.highw.` `.loww.` `.nul.` |
| 文字定数 | `'A'` `'AB'` `'ABCD'`（Shift_JIS 2バイト文字含む） |
| `eval_rpn` | 定数評価・セクション演算・外部参照エラー |
| `.defined.` | シンボル定義チェック演算子 |

### src/addressing/ — EA解析・エンコード（~50件）

| テスト群 | 内容 |
|---|---|
| データレジスタ直接 | `d0`〜`d7` |
| アドレスレジスタ直接 | `a0`〜`a7`, `sp` |
| アドレスレジスタ間接 | `(a0)`, `(a0)+`, `-(a0)` |
| ディスプレースメント | `(4,a0)`, `(-128,a7)` |
| インデックス | `(2,a0,d1.w)`, `(0,a0,d0.l*4)` |
| 絶対アドレス | `$1234.w`, `$100000.l` |
| PC相対 | `(label,pc)`, `(8,pc,d0.w)` |
| 即値 | `#0`, `#$FFFF`, `#%10101010` |

### src/instructions/ — 命令エンコード（~65件）

| テスト群 | 内容 |
|---|---|
| データ転送 | MOVE/MOVEA/MOVEQ/MOVEM/MOVEP/LEA/PEA |
| 算術 | ADD/ADDA/ADDQ/ADDI/ADDX/SUB/SUBA/SUBQ/SUBI/SUBX/CMP/CMPA/CMPI/CMPM/NEG/NEGX/CLR/TST/EXT/SWAP/EXG |
| 乗除算 | MULU/MULS/DIVU/DIVS/CHK/ABCD/SBCD |
| 論理 | AND/OR/EOR/NOT/ANDI/ORI/EORI |
| ビット操作 | BTST/BSET/BCLR/BCHG（静的・動的両形式） |
| シフト/ローテート | ASL/ASR/LSL/LSR/ROL/ROR/ROXL/ROXR（メモリ形式含む） |
| フロー制御 | LINK/UNLK/TRAP/STOP/RTD/BKPT |
| 68020+ | ビットフィールド命令・PACK/UNPK・CAS/CMP2/CHK2 |
| TRAPcc | TRAPF/TRAPT/TRABEQ/TRAPNE 等全バリアント |
| MOVE16 | 68040+ MOVE16 |

### src/symbol/ — シンボルテーブル（~10件）

| テスト群 | 内容 |
|---|---|
| 命令ルックアップ | MOVE/NOP/ADD/SUB 等の InsnHandler 解決 |
| レジスタルックアップ | D0〜D7, A0〜A7, SP, CCR 等 |
| 大文字小文字無視 | `MOVE` と `move` が同一解決 |
| CPU フィルタ | 68000 モードで 68020+ 命令が除外される |

---

## 2. ゴールデンテスト（17件）

### 仕組み

1. `tests/asm/*.s` を rhas でアセンブルして HLK バイナリを生成する。
2. `tests/golden/*.o` に保存された HAS060.X の出力とバイト完全一致を検証する。
3. ゴールデンファイルが存在しない場合はスキップ（`[SKIP]` と表示して `return`）。

### ゴールデンファイルの生成

```bash
# run68 + HAS060.X が必要
zsh tests/gen_golden.sh
```

`gen_golden.sh` の動作:
- `tests/asm/` 内の全 `.s` ファイルを処理する
- ファイル名が `*_opt` で終わる場合は **`-c4`**（拡張最適化）付きで HAS060.X を実行
- それ以外は **`-u -w0`**（未定義→外部参照、警告レベル0）で実行

### テスト一覧

#### 68000命令テスト（`golden_test!` マクロ）

| テスト名 | ソース | 内容 |
|---|---|---|
| `insn_move` | [tests/asm/insn_move.s](../tests/asm/insn_move.s) | MOVE/MOVEA/MOVEQ/MOVEM/MOVEP/LEA/PEA |
| `insn_arith` | [tests/asm/insn_arith.s](../tests/asm/insn_arith.s) | ADD/SUB/CMP/NEG/CLR/EXT/SWAP/EXG/MULU/DIVS 等 |
| `insn_logic` | [tests/asm/insn_logic.s](../tests/asm/insn_logic.s) | AND/OR/EOR/NOT/ANDI/ORI/EORI |
| `insn_bit` | [tests/asm/insn_bit.s](../tests/asm/insn_bit.s) | BTST/BSET/BCLR/BCHG |
| `insn_shift` | [tests/asm/insn_shift.s](../tests/asm/insn_shift.s) | ASL/ASR/LSL/LSR/ROL/ROR/ROXL/ROXR |
| `insn_branch` | [tests/asm/insn_branch.s](../tests/asm/insn_branch.s) | BRA/BSR/Bcc/DBcc/JMP/JSR/RTS/RTE |
| `insn_scc` | [tests/asm/insn_scc.s](../tests/asm/insn_scc.s) | ST/SF/SEQ/SNE/SCC 等 Scc 全バリアント |
| `insn_misc` | [tests/asm/insn_misc.s](../tests/asm/insn_misc.s) | NOP/STOP/RESET/TRAP/LINK/UNLK/ILLEGAL 等 |

#### EAモードテスト

| テスト名 | ソース | 内容 |
|---|---|---|
| `ea_modes` | [tests/asm/ea_modes.s](../tests/asm/ea_modes.s) | 全 EA モードの組み合わせ（12モード） |

#### 疑似命令テスト

| テスト名 | ソース | 内容 |
|---|---|---|
| `pseudo_data` | [tests/asm/pseudo_data.s](../tests/asm/pseudo_data.s) | `.dc` `.ds` `.dcb` `.align` `.even` |
| `pseudo_sym` | [tests/asm/pseudo_sym.s](../tests/asm/pseudo_sym.s) | `.equ` `.set` `.xdef` `.xref` `.globl` `.reg` |
| `pseudo_sect` | [tests/asm/pseudo_sect.s](../tests/asm/pseudo_sect.s) | `.text` `.data` `.bss` `.stack` `.org` `.offset` |
| `pseudo_cond` | [tests/asm/pseudo_cond.s](../tests/asm/pseudo_cond.s) | `.if` `.ifdef` `.ifndef` `.else` `.elseif` `.endif` |
| `pseudo_macro` | [tests/asm/pseudo_macro.s](../tests/asm/pseudo_macro.s) | `.macro` `.endm` `.rept` `.irp` `.irpc` |

#### 式演算テスト

| テスト名 | ソース | 内容 |
|---|---|---|
| `expr_ops` | [tests/asm/expr_ops.s](../tests/asm/expr_ops.s) | `.dc.l` を使った全演算子の式評価 |

#### ROFST・最適化テスト

| テスト名 | ソース | オプション | 内容 |
|---|---|---|---|
| `rofst_disp` | [tests/asm/rofst_disp.s](../tests/asm/rofst_disp.s) | デフォルト | `(const+ext, An)` 逆順パターンが ROFST レコードを生成する |
| `addq_opt` | [tests/asm/addq_opt.s](../tests/asm/addq_opt.s) | `-c4` | `ADD.l #1-8,<ea>` → `ADDQ.l` 変換（`golden_test_opt!`）|

### `golden_test_opt!` マクロ

`-c4`（拡張最適化フラグ全有効）付きでアセンブルするテスト用マクロ。通常の `golden_test!` と区別するため別定義。

```rust
golden_test_opt!(addq_opt);  // assemble_file_c4() を使う
```

`assemble_file_c4()` は以下のフラグを有効化する:

| フラグ | 最適化内容 |
|---|---|
| `opt_adda_suba` | `ADD/SUB #1-8,<ea>` → `ADDQ/SUBQ` |
| `opt_cmpa` | `CMPA` の最適化 |
| `opt_clr` | `CLR` の最適化 |
| `opt_movea` | `MOVEA` の最適化 |
| `opt_lea` / `opt_asl` / `opt_cmp0` / `opt_move0` / `opt_cmpi0` / `opt_sub_addi0` / `opt_bsr` / `opt_jmp_jsr` | 各種最適化 |

---

## 3. 統合テスト（14件）

`tests/integration_test.rs` — 3パス全体を通した end-to-end 検証。

ソーステキストを直接メモリに渡してアセンブルし、生成された HLK バイナリの内容を検証する。

| テスト名 | 検証内容 |
|---|---|
| `test_ms1_move_b_d0_d1` | `move.b d0,d1` が `0x12 0x00` にエンコードされ、HLK 構造が正しい |
| `test_multiple_instructions` | 複数命令のアセンブル（MOVE + ADD） |
| `test_label_and_bra` | ラベル定義と BRA 命令の PC 相対オフセット計算 |
| `test_equ_symbol` | `.equ` シンボル定義と即値置換 |
| `test_section_switch` | `.text` → `.data` → `.bss` セクション切り替えと各セクションサイズ |
| `test_dc_directives` | `.dc.b` `.dc.w` `.dc.l` のバイト出力 |
| `test_ds_directive` | `.ds.b` のバイトカウント記録 |
| `test_conditional_asm` | `.ifdef` / `.ifndef` / `.else` / `.endif` の条件分岐 |
| `test_macro_no_args` | 引数なしマクロ定義・展開 |
| `test_macro_with_args` | 引数付きマクロ（`&param` 置換） |
| `test_rept` | `.rept n` / `.endr` の繰り返し展開 |
| `test_irp` | `.irp param, list` の展開 |
| `test_irpc` | `.irpc param, str` の各文字展開 |
| `test_prn_list_file` | `-p` オプションで PRN リストファイルが生成される |

---

## MS5 対比テスト（HAS ソース直接比較）

HAS060X.X 自身のソースを rhas でアセンブルし、HAS060.X（run68 経由）の出力とバイト比較する。

```bash
# /private/tmp/has_test/compare.sh を参照
SRC_DIR=has_source/src
RHAS=target/debug/rhas
HAS=/private/tmp/has_test/HAS060.X

# -c4 -u フラグで比較
rhas -c4 -u -w0 -I$SRC_DIR $SRC -o $RHAS_O
run68 $HAS -c4 -u -w0 $SRC   # → orig/*.o
diff $ORIG_O $RHAS_O
```

### 2026-02-28 時点の状況（`-c4 -u` 使用）

| ファイル | 状態 | 差分 | 原因 |
|---|---|---|---|
| commitlog.o | ✅ 一致 | 0 | — |
| doasm.o | ⚠️ 差異あり | +170 bytes | 残差（分岐最適化カスケード他） |
| eamode.o | ✅ 一致 | 0 | — |
| encode.o | ✅ 一致 | 0 | — |
| error2.o | ✅ 一致 | 0 | — |
| expr.o | ✅ 一致 | 0 | — |
| fexpr.o | ✅ 一致 | 0 | — |
| file.o | ⚠️ 差異あり | +6 bytes | 残差（分岐カスケード） |
| hupair.o | ✅ 一致 | 0 | — |
| macro.o | ✅ 一致 | 0 | — |
| objgen.o | ⚠️ 差異あり | +4 bytes | 残差（分岐カスケード） |
| opname.o | ✅ 一致 | 0 | — |
| optimize.o | ✅ 一致 | 0 | **修正済み**（ROFST逆順パターン対応で解決） |
| pseudo.o | ⚠️ 差異あり | +52 bytes | 残差 |
| regname.o | ✅ 一致 | 0 | — |
| symbol.o | ✅ 一致 | 0 | — |
| work.o | ✅ 一致 | 0 | — |

13 ファイル一致、4 ファイル差異（error.o / main.o / misc.o は参照ファイルなし）

### 修正履歴（MS5 改善）

| 日付 | 修正内容 | 改善効果 |
|---|---|---|
| 2026-02-28 | `is_external_with_offset` 逆順パターン対応 | optimize.o 完全一致（-192 bytes）、objgen.o -108 bytes、file.o -48 bytes |
| 2026-02-28 | ADD/SUB #1-8 → ADDQ/SUBQ 最適化実装 | doasm.o -6 bytes |

---

## テストファイルの場所

```
tests/
├── asm/               # ゴールデンテスト用アセンブラソース（17本）
│   ├── insn_move.s
│   ├── insn_arith.s
│   ├── ...
│   ├── rofst_disp.s   # ROFST 逆順パターンテスト（2026-02-28追加）
│   └── addq_opt.s     # ADD→ADDQ 最適化テスト（2026-02-28追加）
├── golden/            # HAS060.X の参照出力（.o バイナリ）
│   ├── *.o            # gen_golden.sh で生成
│   └── addq_opt.o     # -c4 付きで生成（HAS060.X -c4 -u -w0）
├── gen_golden.sh      # ゴールデンファイル生成スクリプト
├── golden_test.rs     # ゴールデンテスト実装
└── integration_test.rs # 統合テスト実装
```

---

## 新規テストの追加方法

### ゴールデンテスト（通常オプション）

1. `tests/asm/my_test.s` にアセンブラソースを作成
2. `zsh tests/gen_golden.sh` で `tests/golden/my_test.o` を生成
3. `tests/golden_test.rs` 末尾に `golden_test!(my_test);` を追記

### ゴールデンテスト（`-c4` 付き最適化テスト）

1. `tests/asm/my_feature_opt.s` にアセンブラソースを作成（`_opt` サフィックス必須）
2. `zsh tests/gen_golden.sh` で `-c4` 付きゴールデンを生成
3. `tests/golden_test.rs` 末尾に `golden_test_opt!(my_feature_opt);` を追記

### 統合テスト

`tests/integration_test.rs` に `#[test]` 関数を追加し、`assemble_src(b"...")` を使ってソース直書きでテストする。

---

## 実行コマンド早見表

```bash
# 全テスト実行
cargo test

# ゴールデンファイル再生成（HAS060.X + run68 が必要）
zsh tests/gen_golden.sh

# HAS ソースとの対比確認（compare.sh）
zsh /private/tmp/has_test/compare.sh

# 特定テストのみ実行
cargo test insn_move        # テスト名でフィルタ
cargo test --test golden_test rofst_disp
```
