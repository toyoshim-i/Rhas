# Rhas 実装進捗

## 現在のバージョン情報

- ベース: HAS060X.X v1.2.5 / HAS v3.09+91
- リポジトリ: https://github.com/kg68k/has060xx

---

## フェーズ別進捗

### Phase 1: Foundation（CLIと骨格） ✅ 完了

**目標**: `rhas -h` が動き、ソースファイルを受け取れる

| ファイル | 状態 | 説明 |
|---|---|---|
| `src/options.rs` | ✅ 完了 | HAS060X互換CLIオプション |
| `src/error.rs` | ✅ 完了 | エラーコード + エラー出力 |
| `src/source.rs` | ✅ 完了 | ソースファイル読み込み（Vec<u8>）|
| `src/context.rs` | ✅ 完了 | AssemblyContext骨格 |
| `src/main.rs` | ✅ 完了 | エントリポイント + CLIバインディング |

**参照ファイル**: `external/has060xx/src/main.s`（docmdline, option_*）

---

### Phase 2: シンボルテーブル + 命令テーブル ✅ 完了

**目標**: レジスタ名・命令名が検索できる

| ファイル | 状態 | 説明 |
|---|---|---|
| `src/symbol/types.rs` | ✅ 完了 | シンボル種別 enum（Symbol, SizeFlags, CpuMask, InsnHandler等） |
| `src/symbol/mod.rs` | ✅ 完了 | SymbolTable（HashMap）+ レジスタ名テーブル + 命令テーブル |

**実装内容**:
- `Symbol` enum: Value/Register/Opcode/Macro/Real/RegSym
- `SizeFlags(u8)`: B/W/L/S/D/X/P/Q ビットセット
- `CpuMask(u16)`: SYM_ARCH<<8|SYM_ARCH2 (上位=68k, 下位=ColdFire)
- `InsnHandler` enum: ~80種の命令・疑似命令ハンドラ識別子（Phase 9 拡張分含む）
- `REGISTER_TABLE`: 70+ エントリ（D0-D7, A0-A7, SP, PC, CCR, SR, FPn, CF専用等）
- `OPCODE_TABLE`: 290+ エントリ（全68k命令 + Bcc/DBcc/Scc/JBcc全バリアント + 全疑似命令 + Phase 9 拡張命令）
- 3テーブル構成: `user_syms`（大文字小文字区別）+ `reg_table` + `cmd_table`（区別なし）
- CPU フィルタリング付きルックアップ

**参照ファイル**: `external/has060xx/src/symbol.s`, `symbol.equ`, `regname.s`, `opname.s`

---

### Phase 3: 字句解析 + 式評価 ✅ 完了

**目標**: `1+2*3` や `label+4` を評価できる

| ファイル | 状態 | 説明 |
|---|---|---|
| `src/expr/rpn.rs` | ✅ 完了 | RPNトークン型（Operator/RPNToken/Rpn） |
| `src/expr/mod.rs` | ✅ 完了 | テキスト→RPN変換（シャンティングヤード） |
| `src/expr/eval.rs` | ✅ 完了 | RPN評価（セクション情報付き） |

**実装内容**:
- `Operator` enum: 29演算子（OP_NEG〜OP_OR）、優先順位・単項/二項判定
- `RPNToken` enum: ValueByte/ValueWord/Value/SymbolRef/Location/CurrentLoc/Op/End
- `parse_expr(src, &mut pos)`: シャンティングヤードで直接テキスト→RPN変換
  - 10進・16進($/$0x)・8進(@)・2進(%)リテラル
  - 文字定数 'A'〜'ABCD'（Shift_JIS対応）
  - シンボル参照（評価時に解決）
  - 単項演算子: `-` `+` `~` `.not.` `.high.` `.low.` `.highw.` `.loww.` `.nul.`
  - 二項演算子: `+ - * / .mod. >> << .asr. = <> < <= > >= & ^ |` と全キーワード形式
  - `.defined.` 演算子（シンボル定義チェック）
  - 括弧
- `eval_rpn(rpn, loc, cur_loc, section, lookup)`: スタックベース評価
  - セクション属性付き加減算（<アドレス>±<定数>）
  - 同一セクション <アドレス>-<アドレス> → 定数
  - 異セクション・外部参照 → `DeferToLinker` エラー（Phase 7 で処理）

**参照ファイル**: `external/has060xx/src/expr.s`（convrpn, calcrpn）

---

### Phase 4: 実効アドレス解析 ✅ 完了

**目標**: `(d,An)`, `(d,PC,Dn.l*4)` 等を解析・エンコードできる

| ファイル | 状態 | 説明 |
|---|---|---|
| `src/addressing/mod.rs` | ✅ 完了 | EA型定義（EffectiveAddress/Displacement/IndexSpec等）+ 68000基本12モード解析 |
| `src/addressing/encode.rs` | ✅ 完了 | EAエンコード（6ビットEAフィールド + 拡張ワード生成） |

**実装内容**:
- `EffectiveAddress` enum: DataReg/AddrReg/AddrRegInd/AddrRegPostInc/AddrRegPreDec/AddrRegDisp/AddrRegIdx/AbsShort/AbsLong/PcDisp/PcIdx/Immediate
- `eac` モジュール: EAフィールド値定数（DN/AN/ADR/INCADR/DECADR/DSPADR/IDXADR/ABSW/ABSL/DSPPC/IDXPC/IMM）
- `ea` モジュール: EAビットマスク定数（DATA/MEM/ALT/CTRL/ALL）
- `Displacement`: RPN式 + サイズ指定 + 定数値
- `IndexSpec`: レジスタ番号 + サイズ(.w/.l) + スケール(*1/*2/*4/*8)
- `parse_ea()`: メインAPI（#imm/-(An)/(An)/レジスタ直接/式 → EffectiveAddress）
- `encode_ea()`: EffectiveAddress → EaEncoded（ea_field u8 + ext_bytes Vec<u8>）
- brief拡張ワード生成（68000モード）
- 50+ ユニットテスト

**参照ファイル**: `external/has060xx/src/eamode.s`, `eamode.equ`

---

### Phase 5: 68000基本命令エンコード ✅ 完了

**目標**: 基本的なアセンブルソースからバイト列を生成できる

| ファイル | 状態 | 説明 |
|---|---|---|
| `src/instructions/mod.rs` | ✅ 完了 | 全68000命令エンコーダ（ディスパッチ + 全ハンドラ） |

**実装内容**:
- `encode_insn(base_opcode, handler, size, operands) -> Result<Vec<u8>, InsnError>`
- データ転送: MOVE/MOVEA/MOVEQ/MOVEM/MOVEP/LEA/PEA/JMP/JSR
- 算術: ADD/ADDA/ADDQ/ADDI/ADDX/SUB/SUBA/SUBQ/SUBI/SUBX/CMP/CMPA/CMPI/CMPM/NEG/NEGX/CLR/TST/EXT/SWAP/EXG/MULU/MULS/DIVU/DIVS/CHK/ABCD/SBCD
- 論理: AND/OR/EOR/NOT/ANDI/ORI/EORI
- ビット操作: BTST/BSET/BCLR/BCHG（静的・動的両形式）
- シフト/ローテート: ASL/ASR/LSL/LSR/ROL/ROR/ROXL/ROXR（#imm/Dn/メモリ全形式）
- 分岐: NOP/RTS/RTE等（no-op）、Bcc/DBcc/Scc → DeferToLinker（Phase 7 で解決）
- フロー制御: LINK/UNLK/TRAP/STOP/DEC/INC（HAS独自拡張）
- シンボル参照を含む EA → `InsnError::DeferToLinker`
- 65 ユニットテスト

**参照ファイル**: `external/has060xx/src/doasm.s`（各命令ハンドラ）

---

### Phase 6: 疑似命令（コア） ✅ 完了

**目標**: 実用的なアセンブルソースを処理できる

疑似命令は `src/pass/pass1.rs` に統合実装（別ディレクトリ不使用）

| 疑似命令グループ | 状態 | 説明 |
|---|---|---|
| セクション | ✅ 完了 | `.text` `.data` `.bss` `.stack` `.org` `.offset` `.offsym` |
| データ | ✅ 完了 | `.dc` `.ds` `.dcb` `.align` `.even` `.quad` |
| シンボル | ✅ 完了 | `.equ` `.set` `.reg` `.xdef` `.xref` `.globl` `.comm` `.rcomm` `.rlcomm` |
| 条件 | ✅ 完了 | `.if` `.iff` `.ifdef` `.ifndef` `.else` `.elseif` `.endif` |
| ファイル | ✅ 完了 | `.include` `.insert` `.request` |
| 制御 | ✅ 完了 | `.end` `.cpu` `.fail` |
| リスト制御 | ✅ 完了 | `.list/.nlist` と `.sall/.lall` で PRN 行出力を制御、`.width/.title/.subttl/.page` を PRN へ反映（`.page <expr>` と自動改ページ含む） |

**参照ファイル**: `external/has060xx/src/pseudo.s`

---

### Phase 7: 3パスシステム + オブジェクト生成 ✅ 完了

**目標**: HLKオブジェクトファイルを正しく出力できる

| ファイル | 状態 | 説明 |
|---|---|---|
| `src/pass/mod.rs` | ✅ 完了 | パス制御（assemble エントリポイント） |
| `src/pass/temp.rs` | ✅ 完了 | TempRecord型（30+ バリアント） + 関連型 |
| `src/pass/pass1.rs` | ✅ 完了 | ソース→TempRecord（ラベル解析・命令・疑似命令処理） |
| `src/pass/pass2.rs` | ✅ 完了 | アドレス再計算・分岐最適化 |
| `src/pass/pass3.rs` | ✅ 完了 | TempRecord→オブジェクト（リロケーション処理） |
| `src/object/mod.rs` | ✅ 完了 | HLKオブジェクトフォーマット型定義 |
| `src/object/writer.rs` | ✅ 完了 | HLKバイナリ書き出し |

**実装内容**:
- TempRecord: 30+ バリアント（Const/Branch/RpnData/Ds/Align/SectionChange/XDef/Org/End 等）
- Pass1: ラベル定義/参照収集、全命令・疑似命令の中間コード化
- Pass2: ブランチサイズ縮小、ディスプレースメント縮小、収束ループ
- Pass3: リロケーションテーブル生成、外部参照解決、セクション配置
- HLK writer: $D000ヘッダ、$C0xxセクション、$B2xx外部シンボル、$0000終端
- MS1達成: `move.b d0,d1` → 正しいHLKオブジェクト出力（integration test通過）

**参照ファイル**: `external/has060xx/src/objgen.s`, `docs/hlk_object_format.md`

---

### Phase 8: マクロ処理 ✅ 完了

**目標**: `.macro/.endm`, `.rept`, `.irp/.irpc` が動作する

マクロ処理は `src/pass/pass1.rs` に統合実装

| 機能 | 状態 | 説明 |
|---|---|---|
| `.macro/.endm` 定義 | ✅ 完了 | 引数名マッピング、ローカルラベル収集、テンプレート保存 |
| マクロ展開 | ✅ 完了 | 引数置換（`&param`）、ローカルラベル→`??xxxx`形式 |
| `.rept` | ✅ 完了 | カウント分のボディ繰り返し展開 |
| `.irp` | ✅ 完了 | パラメータリスト分の繰り返し展開 |
| `.irpc` | ✅ 完了 | 文字列の各文字分の繰り返し展開 |

**実装内容**:
- `Symbol::Macro`: `params: Vec<Vec<u8>>` + `local_count: u16` + `template: Vec<u8>`
- テンプレートコンパイル: `&name` → `\xFF idx_hi idx_lo`、`@name` → `\xFE idx_hi idx_lo`
- 展開: `\xFF` marker → 実引数、`\xFE` marker → `??{local_base:04X}{lno:04X}`
- 文字列内の `&param` も置換対応
- 6 integration tests 通過（macro_no_args, macro_with_args, rept, irp, irpc）

**参照ファイル**: `external/has060xx/src/macro.s`

---

### Phase 9: 拡張命令セット ✅ 完了

**目標**: 68010/68020/68030/68040/68060/ColdFire の追加命令をエンコードできる

| 命令グループ | 状態 | 説明 |
|---|---|---|
| 68010追加命令 | ✅ 完了 | RTD/BKPT/MOVES/MOVEC/EXTB |
| ビットフィールド | ✅ 完了 | BFTST/BFCHG/BFCLR/BFSET/BFEXTU/BFEXTS/BFFFO/BFINS（68020+）|
| PACK/UNPK | ✅ 完了 | PACK/UNPK（68020+）|
| CAS/CMP2/CHK2 | ✅ 完了 | CAS/CAS2/CMP2/CHK2（68020+）|
| TRAPcc | ✅ 完了 | TRAPT/TRAPF/TRAPEQ/TRAPNE 等全バリアント |
| MOVE16 | ✅ 完了 | MOVE16（68040+）|
| キャッシュ制御 | ✅ 完了 | CINVL/CINVP/CINVA/CPUSHL/CPUSHP/CPUSHA（68040+）|
| FPU命令 | ⬜ 未着手 | 68881/68882 浮動小数点命令（スコープ外として延期）|

**実装内容**:
- 14 新規 InsnHandler バリアント追加
- 15+ 新規エンコーダ関数（encode_extb, encode_bkpt, encode_trapcc, encode_bitfield_*, encode_moves, encode_movec, encode_packunpk, encode_cas, encode_cmpchk2, encode_move16, encode_cinvpush_lp, encode_cinvpush_a）
- CPUマスクによる命令フィルタリング（68010+/68020+/68040+）

---

### Phase 10: 残り機能 ✅ 完了（主要機能）

| 機能 | 状態 | 説明 |
|---|---|---|
| PRNリストファイル（`-p`） | ✅ 完了 | ソース行+アドレス+機械語バイトのリストファイル生成 |
| シンボルファイル（`-x`） | ✅ 完了 | シンボル名・型・値のリスト出力 |
| `.align` B204レコード | ✅ 完了 | `.align`使用時に `$B204` アラインメント情報レコードを出力 |
| SCD疑似命令（`-g`） | ✅ スタブ | `.def/.endef/.ln/.scl/.type/.size/.val/.dim/.tag` は無視（デバッグシンボル生成は未実装） |
| HUPAIR対応 | N/A | ネイティブRust環境では不要（X68k DOS固有機能） |

**実装内容**:
- `src/pass/prn.rs`: PRN行フォーマッタ（5桁行番号 + 8桁16進アドレス + 機械語バイト + ソーステキスト）
- `TempRecord::LineInfo`: パス3でのPRN行追跡用中間レコード
- `src/pass/mod.rs`: シンボルファイル生成（`format_sym_file`）
- `ctx.max_align` → `obj.has_align/max_align` 伝播修正（`$B204`レコード用）
- 34 integration tests（PRN生成 + `.list/.nlist` + `.sall/.lall` + `.width/.title/.subttl/.page` + `-c4` 最適化 + `.equ/.set`/Pass2回帰 + `-g`検証を含む）通過

---

## マイルストーン

| MS | 達成条件 | 状態 |
|---|---|---|
| MS1 | `move.b d0,d1` → 正しいバイト列、最小限のオブジェクトファイル出力 | ✅ 完了 |
| MS2 | 68000全整数命令エンコード + ラベル・外部参照解決 | ✅ 完了 |
| MS3 | 疑似命令・最適化込みで `HANOI.S` が通る | ✅ 完了（76866 バイト、エラーなし） |
| MS4 | マクロ処理込みで `K_MACRO.MAC` が通る | ✅ 完了（エラーなし、構造化マクロライブラリ全定義処理）|
| MS5 | 実X68000プログラムのビルドがオリジナルと完全一致 | ✅ 完了（17ファイル中17一致） |
| MS6 | FPU/ColdFire/SCD/PRN全機能 | ⬜ |

---

## テスト状況

| テストスイート | 件数 | 状態 |
|---|---|---|
| ユニットテスト（src内 #[cfg(test)]） | 多数 | ✅ 全通過 |
| 統合テスト（tests/integration_test.rs） | 34件 | ✅ 全通過 |
| ゴールデンテスト（tests/golden_test.rs） | 17件 | ✅ 全通過 |

---

## 設計方針（決定済み）

| 項目 | 決定内容 |
|---|---|
| 中間表現 | メモリ上 `Vec<TempRecord>` |
| 文字コード | バイト列 `Vec<u8>`（変換なし） |
| CLI | HAS060Xと完全互換 |
| テスト | エミュレータでオリジナルを動かして `.o` をバイト比較 |
| シンボルテーブル | `HashMap<Vec<u8>, Symbol>`（大文字小文字区別なし比較） |
| ハッシュ関数 | オリジナルと揃えなくてよい（内部実装） |
| 疑似命令・マクロ | 別ディレクトリではなく pass1.rs に統合実装 |

---

## 変更ログ

### 2026-02-28

- MS5差分の追加調査と実装見直し
  - `src/pass/temp.rs`: `TempRecord::Branch` に `cur_size` / `suppressed` を追加
  - `src/pass/pass2.rs`: 自動分岐のサイズ再判定と直後 `BRA/Bcc` サプレス処理を追加
  - `src/pass/pass3.rs`: 分岐サプレス状態を反映して出力
  - `src/pass/pass1.rs`: `opt_asl`（`ASL #1,Dn -> ADD Dn,Dn`）実装、`jmp/jsr` 最適化条件をオリジナル寄りに調整
  - `tests/integration_test.rs`: 4件追加（直後BRAサプレス + `-c4` 最適化3件）
- Pass2 の見直し（DeferredInsn のサイズ再評価）
  - `src/pass/pass2.rs`: 未解決 EA を Pass2 で再評価し、`DeferredInsn.byte_size` とラベル値再計算に反映
  - `tests/integration_test.rs`: 回帰テスト `test_pass2_updates_labels_after_deferred_size_change` を追加
    - 既知不具合: `bra target` が `6004` になるケースを `6002` に修正
- 数値ローカルラベル（`1f` / `1b`）の実装
  - `src/pass/pass1.rs`: `1:` 定義と `1f`/`1b` 参照を一意名へ前処理展開
  - `tests/integration_test.rs`: 前方/後方参照の回帰テスト 2件を追加
- 数値ローカルラベル展開の安全化
  - `src/pass/pass1.rs`: `$2b` など数値リテラル、およびクォート文字列内を置換対象から除外
  - `tests/integration_test.rs`: `test_numeric_local_label_does_not_touch_hex_literal` を追加
- Pass3 外部式判定の一般化（ROFST化）
  - `src/pass/pass3.rs`: `is_external_with_offset` を定数畳み込み付きに拡張（`sym + (16*4)` などを `xref + offset` として扱う）
  - `src/pass/pass3.rs` unit test 追加: `test_is_external_with_offset_mul_add_const_fold`
- `.equ/.set` とマクロローカルラベルの見直し
  - `src/pass/temp.rs`: `TempRecord::EquDef` を追加
  - `src/pass/pass2.rs`: `.equ/.set` を反復再評価してラベル再配置後の値に追従
  - `src/pass/pass1.rs`: 行頭 `*` の評価位置同期、ロケーション依存 `.equ/.set` を `NoDet` 扱い
  - `src/pass/pass1.rs`: マクロ定義収集時に `@name` ローカルラベル置換を常時実施（引数なしマクロ含む）
  - `src/pass/pass1.rs`: `.dc` のシンボル/ロケーション依存式は Pass3 で最終評価するよう変更
  - `tests/integration_test.rs`: 追加
    - `test_equ_location_counter_uses_line_top`
    - `test_dc_label_diff_recomputed_after_pass2`
- 動的 `.equ` を含む命令の早期 `Const` 固定を抑制
  - `src/pass/pass1.rs`: `DeferToLinker` 再エンコード時、動的参照を含むEAは `DeferredInsn` のまま保持
  - `tests/integration_test.rs`: `test_addq_immediate_from_dynamic_equ_not_frozen_in_pass1` を追加
- `-g` 指定時の `$B204` 出力を実装
  - `src/object/mod.rs`: `ObjectCode::has_debug_info` を追加
  - `src/pass/mod.rs`: `-g` オプションを `ObjectCode` へ伝播
  - `src/object/writer.rs`: `.align` 未使用でも `-g` なら `$B204` を出力
  - `tests/integration_test.rs`: `test_g_option_emits_b204_record` を追加
- `.request` の `$E001` 出力を実装
  - `src/pass/pass1.rs`: `.request` ファイル名を収集
  - `src/pass/mod.rs`: 収集した request ファイル名を `ObjectCode` へ伝播
  - `src/object/writer.rs`: `$E001` レコードを出力
  - `tests/integration_test.rs`: `test_request_emits_e001_record` を追加
- PRN `.list/.nlist` 行制御を修正
  - `src/pass/pass1.rs`: `.nlist` 行は当該行から非表示となるよう先読み判定を追加
  - `src/context.rs`: `AssemblyContext::prn_listing` フラグを追加
  - `tests/integration_test.rs`: `test_prn_nlist_and_list` を追加
- PRN `.sall/.lall` マクロ行制御を実装
  - `src/context.rs`: `AssemblyContext::prn_macro_listing` フラグを追加
  - `src/pass/pass1.rs`: `.sall/.lall` でマクロ展開行の `LineInfo` 出力を切替
  - `tests/integration_test.rs`: `test_prn_lall_shows_macro_expansion_lines` を追加
- PRN `.width` と `-f` 設定の反映を実装
  - `src/pass/pass1.rs`: `.width` の定数評価（80..255, 8刻み丸め）を実装
  - `src/pass/prn.rs`: `line_width/code_width` を受け取る可変フォーマッタ化
  - `src/pass/mod.rs`: `Options.prn_width/prn_code_width` を `format_prn` へ伝播
  - `tests/integration_test.rs`: `test_prn_width_directive_limits_line_width` を追加
- PRN `.title/.subttl` 反映を実装
  - `src/pass/pass1.rs`: `.title/.subttl` 文字列を解析して `AssemblyContext` へ保持
  - `src/pass/prn.rs`: PRN先頭に `TITLE/SUBTTL` ヘッダ行を出力
  - `src/pass/mod.rs`: `ctx.prn_title/ctx.prn_subttl` を `format_prn` へ伝播
  - `tests/integration_test.rs`: `test_prn_title_and_subttl_are_reflected` を追加
- PRN `.page` 反映を実装
  - `src/pass/prn.rs`: `.page` 行を検出し、ページング有効時にフォームフィード（0x0C）を出力
  - `src/pass/mod.rs`: `Options.prn_no_page_ff` を `format_prn` へ伝播
  - `tests/integration_test.rs`: `test_prn_page_emits_formfeed_unless_disabled` を追加
- `.page <expr>` の行数設定を実装
  - `src/pass/pass1.rs`: `.page <expr>` を `prn_page_lines` 更新として扱い、`.page`/`.page +` と分離
  - `src/pass/prn.rs`: 改ページ判定を `.page`/`.page +` のみに限定
  - `src/pass/mod.rs`: `Options.prn_page_lines` を `format_prn` へ伝播
  - `tests/integration_test.rs`: `test_prn_page_with_expr_sets_page_lines_without_formfeed` を追加
- `prn_page_lines` による自動改ページを実装
  - `src/pass/prn.rs`: 1ページ当たり出力行数を計測し、到達時にフォームフィード（0x0C）を出力
  - `tests/integration_test.rs`: `test_prn_auto_page_break_by_line_limit` を追加
- 検証結果
  - `cargo test --test golden_test`: 17/17 通過
  - `cargo test --test integration_test`: 34/34 通過
  - `tests/compare_ms5_simple.sh`: 17一致 / 0差分

### 2026-02-24

- Phase 10 完了（主要機能）
  - `src/pass/prn.rs`: PRNリストファイル生成（`format_prn`）
  - `src/pass/temp.rs`: `TempRecord::LineInfo` 追加
  - `src/pass/pass3.rs`: PRN行追跡 + シグネチャ更新（prn_enable, max_align）
  - `src/pass/mod.rs`: PRN + シンボルファイル書き出し + `format_sym_file`
  - `src/pass/pass1.rs`: `.align`使用時の `ctx.max_align` 更新
  - 180ユニットテスト + 14統合テスト全通過

- Phase 9 完了
  - `src/symbol/types.rs`: 14 新規 InsnHandler バリアント追加
  - `src/symbol/mod.rs`: 60+ 新規 OPCODE_TABLE エントリ（68010-68040+命令）
  - `src/instructions/mod.rs`: 15+ 新規エンコーダ関数（ビットフィールド・MOVEC・MOVES・CAS・CMP2/CHK2・MOVE16・CINV*/CPUSH*等）
  - 177ユニットテスト + 13統合テスト全通過

- Phase 8 完了
  - `src/pass/pass1.rs`: マクロ定義・展開・.rept/.irp/.irpc 実装
  - `src/symbol/types.rs`: Symbol::Macro に params フィールド追加
  - 177ユニットテスト + 13統合テスト全通過（マクロ統合テスト6件含む）

- Integration tests 追加
  - `tests/integration_test.rs`: 13件の統合テスト（MS1含む全パイプライン検証）
  - `src/lib.rs`: ライブラリクレートとして公開（テスト用）
  - `Cargo.toml`: lib section + tempfile dev dependency 追加

- Phase 5〜7 完了（前セッションより）
  - `src/instructions/mod.rs`: 全68000命令エンコーダ
  - `src/pass/`: 3パスシステム + TempRecord + HLKオブジェクト生成
  - `src/object/`: HLKフォーマット型定義 + バイナリ書き出し
  - 177テスト全通過

- Phase 2〜4 完了（前セッションより）
  - `src/symbol/`: シンボルテーブル、型定義、命令テーブル
  - `src/expr/`: RPN変換・評価
  - `src/addressing/`: EAモード解析・エンコード

- Phase 1 完了
  - `src/options.rs`: HAS060X互換CLIパーサ実装（全スイッチ対応）
  - `src/error.rs`: エラー型・ワーニング型・エラー出力実装
  - `src/source.rs`: ソースファイル読み込み実装
  - `src/context.rs`: AssemblyContext骨格実装
  - `src/main.rs`: エントリポイント実装

---

## 参照ドキュメント

- [HASアーキテクチャ](has_architecture.md)
- [Human68kシステムコール](human68k_syscalls.md)
- [HLKオブジェクトフォーマット](hlk_object_format.md)
- [M68000アドレッシングモード](m68000_addressing.md)
