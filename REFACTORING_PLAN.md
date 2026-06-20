# Rhas リファクタリング計画（段階的・テスト駆動）

## 🎯 全体方針
- **各ステップは最小限の変更** に留める
- **各ステップ後に必ず `cargo test --lib` を実行** してリグレッション確認
- **既存機能に一切影響しない範囲** での改善を優先
- テスト数 184 個が全てパスしている状態を維持

**ベースライン**: `cargo test --lib` → 184 test passed ✅

---

## 📋 実装済みテスト環境
```bash
# ユニットテスト実行（約0.07秒）
cargo test --lib

# 統合テスト実行
cargo test --test integration_test
cargo test --test golden_test
cargo test --test error_message_test

# 全テスト実行
cargo test
```

---

## 🔄 Step 1: ユーティリティ関数統一 (`utils/mod.rs` 作成)

### 目的
重複している以下の関数を一箇所に統一：
- `String::from_utf8_lossy()` 呼び出し（20+ 箇所）
- `Vec::with_capacity()` 初期化パターン（バイト列用、式用など）
- `to_lowercase()` / `to_lowercase_vec()` → 統一インターフェース

### 対象ファイル
- `src/utils/mod.rs` (新規作成)
- `src/lib.rs` (モジュール追加)

### 変更内容
```rust
// src/utils/mod.rs (新規)
pub fn bytes_to_string(b: &[u8]) -> String
pub fn path_from_bytes(b: &[u8]) -> PathBuf
pub fn to_lowercase_vec(s: &[u8]) -> Vec<u8>
pub fn vec_with_capacity_for_bytes(estimate: usize) -> Vec<u8>
pub fn vec_with_capacity_for_rpn() -> Vec<RPNToken>
```

### テスト方法
```bash
# Step 1a: utils/mod.rs → 新規関数群を追加
cargo test --lib
# 確認: 184 passed

# Step 1b: 各モジュールで新関数を利用（置き換え）
# options.rs, error.rs, main.rs などで置き換え実施
cargo test --lib
# 確認: 184 passed

# Step 1c: symbol/mod.rs の to_lowercase_vec を削除
cargo test --lib
# 確認: 184 passed
```

### 完了基準
- ✅ `cargo test --lib` で 184 passed のまま
- ✅ `String::from_utf8_lossy()` の呼び出し回数が減少
- ✅ 既存機能に影響なし

---

## 🔄 Step 2: デッドコード・サプレッション見直し

### 目的
実装完了の見通しがつかないコードの `#![allow(dead_code)]` を削除し、
アクティブに使われているのか、本当に不要なのかを整理。

### 対象ファイル（優先順）
1. `src/error.rs` - いくつかのエラーコードは実装予定か確認
2. `src/options.rs` - 複数オプションが未使用か確認
3. `src/expr/mod.rs` - `ParseError::Internal` 等の不要属性
4. `src/instructions/mod.rs` - テスト用ヘルパ `encode_ok`

### 変更内容
```bash
# Step 2a: error.rs の #![allow(dead_code)] を削除し、実際の未使用を確認
cargo check
# → コンパイル警告が出れば、本当に未使用か手動確認

# Step 2b: 他モジュールの `#[allow(dead_code)]` も逐次削除
# (expr/mod.rs, instructions/mod.rs など)
cargo test --lib
# 確認: 184 passed
```

### 完了基準
- ✅ `cargo test --lib` で 184 passed のまま
- ✅ デッドコードの状況が明確に文書化
- ✅ 未使用コードに対する警告が出力されるようになった
- ✅ STEP 2 完了（2026-03-08）

### 実施結果
Step 2 では当初想定の14ファイルからさらに `expr/mod.rs` と `instructions/mod.rs` に
存在していた `#[allow(dead_code)]` を除去。テスト 229 すべて通過、警告は残るが
`dead_code` 属性がリポジトリから消えた。
```
---

## 🔄 Step 3: `pass/pass1.rs` の疑似命令処理分割

### 目的
`handle_pseudo()` 関数（現在 ~1000 行）を以下に分割：
- `src/pass/pseudo_handler.rs` - セクション、.dc, .ds 等
- `src/pass/pseudo_conditional.rs` - .if/.ifdef/.endif
- `src/pass/pseudo_macro.rs` - .macro/.rept/.irp 等

### 対象ファイル
- `src/pass/pass1.rs` (2500 行 → 1500 行に削減目標)
- `src/pass/mod.rs` (モジュール公開)

### 変更内容
```rust
// src/pass/pseudo_handler.rs (新規)
pub mod section;          // .text, .data など
pub mod pseudo_data;      // .dc, .ds, .dcb
pub mod pseudo_conditional;  // .if, .ifdef, .else, .endif
pub mod pseudo_macro;     // .macro, .rept, .irp
pub mod pseudo_debug;     // SCD デバッグ疑似命令

// pass1.rs の handle_pseudo() 内部：
pub fn dispatch_pseudo_handler(
    handler: InsnHandler,
    mnem: &[u8],
    size: Option<SizeCode>,
    line: &[u8],
    pos: &mut usize,
    label: &Option<Vec<u8>>,
    records: &mut Vec<TempRecord>,
    p1: &mut P1Ctx<'_>,
    source: &mut SourceStack,
) {
    match handler {
        InsnHandler::TextSect | InsnHandler::DataSect => 
            section::handle_section(handler, p1, records),
        InsnHandler::Dc | InsnHandler::Ds | InsnHandler::Dcb =>
            pseudo_data::handle_data(handler, size, line, pos, p1, records),
        InsnHandler::If | InsnHandler::Ifdef =>
            pseudo_conditional::handle_if(handler, line, pos, p1),
        InsnHandler::MacroDef =>
            pseudo_macro::handle_macro_def(label, line, pos, source, p1),
        ...
    }
}
```

### テスト方法
```bash
# Step 3a: 新ファイル群を作成（stub 実装）
cargo check
# エラーがないか確認

# Step 3b: section 処理を分割
# pass1.rs の InsnHandler::TextSect 以降を削除、pseudo_handler::section へ委譲
cargo test --lib
# 確認: 184 passed

# Step 3c: data 処理を分割 (.dc, .ds, .dcb)
cargo test --lib
# 確認: 184 passed

# Step 3d: 条件分岐処理を分割 (.if, .ifdef, .else, .endif)
cargo test --lib
# 確認: 184 passed

# Step 3e: マクロ処理を分割 (.macro, .rept, .irp)
cargo test --lib
# 確認: 184 passed

# Step 3f: SCD デバッグ処理を分割
cargo test --lib
# 確認: 184 passed
```

### 完了基準
- ✅ `cargo test --lib` で 184 passed のまま
- ✅ `src/pass/pass1.rs` が 3500 行以下に削減
- ✅ 各疑似命令カテゴリが独立したモジュール
- ✅ 保守性向上（新規疑似命令追加が容易）

---

## 🔄 Step 4: エラー/警告処理の型安全化

### 目的
`ErrorCode` enum と `WarnCode` enum を より型安全に活用：
- エラーメッセージの文字列化時に型チェック
- エラーコンテキスト（ソース位置）の構造化

### 対象ファイル
- `src/error.rs` (拡張)

### 変更内容
```rust
// Currently:
print_error(&mut stderr, &self.current_pos, code, sym);

// After:
#[derive(Debug, Clone)]
pub struct ErrorContext {
    pub pos: SourcePos,
    pub symbol: Option<Vec<u8>>,
}

pub fn report_error(ctx: &mut ErrorContext, code: ErrorCode) {
    ctx.print_to_stderr();
}
```

### テスト方法
```bash
# Step 4a: ErrorContext 型導入
cargo test --lib
# 確認: 184 passed

# Step 4b: 既存呼び出しを ErrorContext に変更
cargo test --lib
# 確認: 184 passed
```

### 完了基準
- ✅ `cargo test --lib` で 184 passed のまま  
- ✅ エラーレポートの型チェック向上
- ✅ ログ出力がより構造化

---

## 🔄 Step 5: CPU タイプ定義の統一

### 目的
CPU タイプが `u32` (number) と `u16` (bitflag) に分散しているのを統一：

```rust
// Before: 混在
ctx.cpu_number: u32 = 68000
ctx.cpu_type: u16 = 0x0100

// After: 統一された型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CpuType {
    pub number: u32,      // 68000, 68010, ...
    pub features: u16,    // bitflag: C000, C010, ...
}
```

### 対象ファイル
- `src/context.rs`
- `src/options.rs`
- `src/pass/pass1.rs` (CPU setters の更新)

### テスト方法
```bash
# Step 5a: CpuType struct 定義
cargo check

# Step 5b: AssemblyContext で使用開始
cargo test --lib
# 確認: 184 passed

# Step 5c: 既存コード内の cpu_number/cpu_type をリファクタ
cargo test --lib
# 確認: 184 passed
```

### 完了基準
- ✅ `cargo test --lib` で 184 passed のまま
- ✅ CPU 情報が單一の構造体で管理
- ✅ 型安全性向上

---

## 🔄 Step 6: `pass/pass3.rs` のモジュール分割・整理

### 目的
`pass3.rs`（現在約1500行）をサブモジュールに分割して見通しを改善する。主に実効アドレス（EA）解決ロジックや分岐・命令エンコード処理を分割する。

### 対象ファイル
- `src/pass/pass3.rs` (本体を軽量化)
- `src/pass/pass3/mod.rs` (新規)
- `src/pass/pass3/ea.rs` (新規: EA解決・リロケーション関連)
- `src/pass/pass3/branch.rs` (新規: 分岐命令・DBcc/FBcc/FDBcc の解決)
- `src/pass/pass3/insn.rs` (新規: 未解決命令のエンコード委譲処理)

### 変更内容
`pass3.rs` を `pass3` ディレクトリ下の `mod.rs` にし、以下のようにモジュールを分割する。

- `src/pass/pass3/ea.rs`: `resolve_ea_with_ext`, `ea_ext_size_for_insn` などの EA / 外部参照解析関連ヘルパ関数群を配置する。
- `src/pass/pass3/branch.rs`: `process_branch` などの分岐命令ハンドリングロジックを配置する。
- `src/pass/pass3/insn.rs`: `encode_insn` の呼び出しとフォールバックハンドリングなど命令エンコードに関連するロジックを配置する。
- `src/pass/pass3/mod.rs` (旧 `pass3.rs`): `pass3()` エントリポイントと `P3Ctx` 定義、主要な中間コードレコードディスパッチャ（レコードループ）のみに集中させる。

### テスト方法
```bash
# 各モジュールをスタブで追加した後に徐々に移行する
cargo test
# 確認: 98 passed (integration), 63 passed (golden)
```

### 完了基準
- ✅ `cargo test` ですべてのテストがパスすること
- ✅ `src/pass/pass3.rs` のファイルサイズが 500 行以下に削減されること
- ✅ 各モジュールの責務が明確になり、循環参照が発生しないこと

---

## 🔄 Step 7: `pass/pass1.rs` の命令・オペランドパースの分割

### 目的
`pass1.rs`（約2300行）をサブモジュール化して見通しを改善する。特に複雑なオペランドパース（FPUレジスタリストやペアのパースなど）と、命令サイズ・アドレッシングモード検証ロジックを分割する。

### 対象ファイル
- `src/pass/pass1.rs` (本体を軽量化)
- `src/pass/pass1/mod.rs` (新規: `pass1` エントリポイント)
- `src/pass/pass1/operand.rs` (新規: `parse_operands` および各種 FPU/レジスタパースヘルパー)
- `src/pass/pass1/insn.rs` (新規: `handle_real_insn`, `estimate_insn_size` などの実命令解析処理)
- `src/pass/pass1/preprocess.rs` (新規: `preprocess_anon_labels`, `preprocess_numeric_local_labels`)

### 変更内容
`pass1.rs` を `pass1` ディレクトリ下の `mod.rs` にし、以下のようにモジュールを分割する。

- `src/pass/pass1/operand.rs`: `parse_operands`, `parse_fp_reg_list_token`, `parse_fp_ctrl_list_token`, `parse_fp_pair_token`, `parse_fp_register_token` などのオペランドパーサー群を配置。
- `src/pass/pass1/insn.rs`: `handle_real_insn`, `estimate_insn_size`, `resolve_ea_const_for_size` などの命令デコード・サイズ見積もり処理を配置。
- `src/pass/pass1/preprocess.rs`: `preprocess_anon_labels`, `preprocess_numeric_local_labels` などのアセンブル前ラベル正規化ヘルパー群を配置。
- `src/pass/pass1/mod.rs` (旧 `pass1.rs`): `pass1` エントリポイントおよびメインループ、行パース `parse_line` とディレクティブ分岐処理 `handle_pseudo` のディスパッチ処理のみに集中させる。

### テスト方法
```bash
cargo test
# 確認: すべてのテストがパスすること
```

### 完了基準
- ✅ `cargo test` ですべてのテストがパスすること
- ✅ `src/pass/pass1.rs` (現在の `mod.rs`) のファイルサイズが 800 行以下に削減されること
- ✅ 各モジュールの責務が明確になり、循環参照が発生しないこと

---

## 🔄 Step 8: `symbol/mod.rs` のテーブル分割・整理 (built-in データの分離)

### 目的
`src/symbol/mod.rs` (約1100行) から、静的な命令定義テーブル (`OPCODE_TABLE`) やレジスタ定義テーブル (`REGISTER_TABLE`) などの built-in データを別ファイル (`table.rs`) に分割し、シンボルテーブル管理のコアロジックを読みやすく簡潔にする。

### 対象ファイル
- `src/symbol/mod.rs` (本体を軽量化)
- `src/symbol/table.rs` (新規: built-in テーブルデータ定義)

### 変更内容
`REGISTER_TABLE`, `OPCODE_TABLE`, `OpcodeEntry`, `RegEntry` および関連する定数群を `src/symbol/table.rs` へ移行する。

### テスト方法
```bash
cargo test
# 確認: すべてのテストがパスすること
```

### 完了基準
- ✅ `cargo test` ですべてのテストがパスすること
- ✅ `src/symbol/mod.rs` のファイルサイズが 400 行以下に削減されること

---

## 🔄 Step 9: `options.rs` のモジュール分割・整理

### 目的
`src/options.rs`（現在約980行）を `src/options/` ディレクトリ配下に分割・整理し、コマンドライン引数の定義、CPU種別、パース処理の責務を分離することで可読性と保守性を向上させる。

### 対象ファイル
- `src/options.rs` (削除)
- `src/options/mod.rs` (新規: エントリポイント)
- `src/options/cpu.rs` (新規: CPU種別定義)
- `src/options/types.rs` (新規: Options構造体定義)
- `src/options/parser.rs` (新規: 引数パース処理)

### テスト方法
```bash
cargo test
# 確認: すべてのテストがパスすること
```

### 完了基準
- ✅ `cargo test` ですべてのテストがパスすること
- ✅ `src/options.rs` が削除され、`src/options/mod.rs` が 150 行以下に削減されること

---

## 🔄 Step 10: `source.rs` のモジュール分割・整理

### 目的
`src/source.rs`（現在約360行）を `src/source/` ディレクトリ配下に分割・整理し、ソースバッファ表現 (`SourceBuf`)、インクルードスタック管理 (`SourceStack`)、パス解決ユーティリティの責務を分離することで可読性と保守性を向上させる。

### 対象ファイル
- `src/source.rs` (削除)
- `src/source/mod.rs` (新規: エントリポイント)
- `src/source/buf.rs` (新規: ソースバッファ構造)
- `src/source/stack.rs` (新規: インクルードスタック処理)
- `src/source/path.rs` (新規: インクルードパス変換)

### テスト方法
```bash
cargo test
# 確認: すべてのテストがパスすること
```

### 完了基準
- ✅ `cargo test` ですべてのテストがパスすること
- ✅ `src/source.rs` が削除され、`src/source/mod.rs` が 100 行以下に削減されること

---

## 📊 進捗チェックリスト

| Step | 説明 | 状態 | テスト結果 | 実施日 |
|------|------|------|-----------|--------|
| 0 | ベースライン | ✅ | 184/184 | 2026-03-07 |
| 1 | ユーティリティ統一 | ✅ | 173/173 | 2026-05-22 |
| 2 | デッドコード見直し | ✅ | 229/229 (no change) | 2026-03-08 |
| 3 | pass1 分割 | ✅ | 229/229 | 2026-04-01 |
| 4 | エラー型安全化 | ✅ | 13/13 (lib), 63/63 (golden), 98/98 (integration) | 2026-05-23 |
| 5 | CPU 型統一 | ✅ | 13/13 (lib), 63/63 (golden), 98/98 (integration) | 2026-05-23 |
| 6 | pass3 分割 | ✅ | 13/13 (lib), 63/63 (golden), 98/98 (integration) | 2026-05-24 |
| 7 | pass1 再分割（命令・オペランドパース） | ✅ | 13/13 (lib), 63/63 (golden), 98/98 (integration) | 2026-05-24 |
| 8 | symbol built-in テーブル分離 | ✅ | 13/13 (lib), 63/63 (golden), 98/98 (integration) | 2026-05-25 |
| 9 | options 分割 | ✅ | 13/13 (lib), 63/63 (golden), 98/98 (integration) | 2026-05-25 |
| 10 | source 分割 | ✅ | 13/13 (lib), 63/63 (golden), 98/98 (integration) | 2026-05-26 |
| 11 | error 分割 | ✅ | 13/13 (lib), 63/63 (golden), 98/98 (integration) | 2026-05-26 |
| 12 | context 分割 | ✅ | 13/13 (lib), 63/63 (golden), 98/98 (integration) | 2026-05-26 |
| 13 | instructions 分割 | ✅ | 13/13 (lib), 63/63 (golden), 98/98 (integration) | 2026-05-30 |
| 14 | addressing 分割 | ✅ | 13/13 (lib), 63/63 (golden), 98/98 (integration) | 2026-05-31 |
| 15 | expr 分割 | ✅ | 13/13 (lib), 63/63 (golden), 98/98 (integration) | 2026-05-31 |
| 16 | pass2 分割 | ✅ | 13/13 (lib), 63/63 (golden), 98/98 (integration) | 2026-05-31 |
| 17 | writer 分割 | ✅ | 13/13 (lib), 63/63 (golden), 98/98 (integration) | 2026-06-01 |
| 18 | pass1_macro 分割 | ✅ | 13/13 (lib), 63/63 (golden), 98/98 (integration) | 2026-06-03 |
| 19 | pass1_pseudo 分割 | ✅ | 13/13 (lib), 63/63 (golden), 98/98 (integration) | 2026-06-04 |
| 20 | instructions ユニットテスト分離 | ✅ | 13/13 (lib), 63/63 (golden), 98/98 (integration) | 2026-06-08 |
| 21 | pass1_optimize 分割 | ✅ | 13/13 (lib), 63/63 (golden), 98/98 (integration) | 2026-06-10 |
| 22 | instructions/ops.rs カテゴリ分割 | ✅ | 13/13 (lib), 63/63 (golden), 98/98 (integration) | 2026-06-11 |
| 23 | addressing ユニットテスト分離 | ✅ | 13/13 (lib), 63/63 (golden), 98/98 (integration) | 2026-06-12 |
| 24 | expr ユニットテスト分離 | ✅ | 13/13 (lib), 63/63 (golden), 98/98 (integration) | 2026-06-14 |
| 25 | symbol ユニットテスト分離 | ✅ | 13/13 (lib), 63/63 (golden), 98/98 (integration) | 2026-06-14 |
| 26 | pass/pseudo ユニットテスト分離 | ✅ | 13/13 (lib), 63/63 (golden), 98/98 (integration) | 2026-06-14 |
| 27 | pass/prn ユニットテスト分離 | ✅ | 13/13 (lib), 63/63 (golden), 98/98 (integration) | 2026-06-14 |
| 28 | object/writer ユニットテスト分離 | ✅ | 13/13 (lib), 63/63 (golden), 98/98 (integration) | 2026-06-17 |
| 29 | options ユニットテスト分離 | ✅ | 13/13 (lib), 63/63 (golden), 98/98 (integration) | 2026-06-17 |
| 30 | source ユニットテスト分離 | ✅ | 13/13 (lib), 63/63 (golden), 98/98 (integration) | 2026-06-18 |
| 31 | utils ユニットテスト分離 | ✅ | 13/13 (lib), 63/63 (golden), 98/98 (integration) | 2026-06-20 |






---

## 🚀 実施上のポイント

### テスト実行コマンド（毎ステップ後）
```bash
# ユニットテスト（推奨、高速）
cargo test --lib

# 統合テストも含める（時間がある場合）
cargo test

# 特定テストだけ実行（デバッグ時）
cargo test symbol::tests::test_lookup_opcode_move
```

### Git 運用
```bash
# 各ステップをfeatureブランチで実施
git checkout -b refactor/step-1-utils
# ... 変更 ...
cargo test --lib  # 確認
git commit -m "refactor: unify utility functions"

git checkout -b refactor/step-2-deadcode
# ...
```

### ロールバック方法
```bash
# テスト失敗時は該当ステップをやり直し
git checkout refactor/step-X
git reset --hard origin/refactor/step-X
```

---

## ⚠️ 注意事項

1. **各ステップは独立させる** - 前のステップが完了するまで次へ進まない
2. **テスト失敗 = すぐロールバック** - リグレッションは許さない
3. **大型ファイル分割では import パス注意** - 循環参照を避ける
4. **ドキュメント更新** - リファクタ後は README.md に反映検討

---

## 📅 予想実施期間

| Step | 所要時間 | 難易度 |
|------|----------|--------|
| 1 | 30-45 分 | ⭐☆☆ |
| 2 | 20-30 分 | ⭐☆☆ |
| 3 | 2-3 時間 | ⭐⭐⭐ |
| 4 | 1-1.5 時間 | ⭐⭐☆ |
| 5 | 1-1.5 時間 | ⭐⭐☆ |
| 6 | 2-3 時間 | ⭐⭐⭐ |
| 7 | 2-3 時間 | ⭐⭐⭐ |
| 8 | 45-60 分 | ⭐☆☆ |
| **合計** | **10-15 時間** | |

---

## 🎓 期待される効果

✅ **コード保守性向上**
- 関数の責任が明確に → テスト追加が容易
- ファイルサイズ削減 → 理解しやすく

✅ **開発速度向上**
- 重複コードがない → バグ修正の影響範囲が狭い
- 一箇所の修正で複数箇所に効果

✅ **品質向上**
- 型安全性向上 → IDE 補完強化
- テスト数維持 → リグレッション防止

---

**作成日**: 2026-03-07  
**版**: 1.0
