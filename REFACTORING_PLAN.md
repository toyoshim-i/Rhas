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

## 📊 進捗チェックリスト

| Step | 説明 | 状態 | テスト結果 | 実施日 |
|------|------|------|-----------|--------|
| 0 | ベースライン | ✅ | 184/184 | 2026-03-07 |
| 1 | ユーティリティ統一 | ⏳ | - | - |
| 2 | デッドコード見直し | ✅ | 229/229 (no change) | 2026-03-08 |
| 3 | pass1 分割 | ⏳ | - | - |
| 4 | エラー型安全化 | ⏳ | - | - |
| 5 | CPU 型統一 | ⏳ | - | - |

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
| **合計** | **5-8 時間** | |

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
