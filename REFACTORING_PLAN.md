# Rhas リファクタリング計画（第2期）

本計画は、ユニットテストの分離作業（Step 1〜34）の完了に伴い、残るコード負債の解消とアーキテクチャの強化を目的とした新たな3つのリファクタリングフェーズを定義します。

---

## 📅 ロードマップ概要

- **フェーズ1**: 巨大な統合テスト (`tests/integration_test.rs`) の機能別ファイル分割
- **フェーズ2**: 未使用コード（Clippy警告）の精密調査とクリーンアップ（移植漏れの検証を含む）
- **フェーズ3**: エラー・警告報告処理の統一と抽象化 (`ErrorReporter` の導入)

---

## 🧪 フェーズ1: 巨大な統合テストの機能別分割

### 目的
約2300行、98件のテストが1つのファイル `tests/integration_test.rs` に混在しており、メンテナンス性が低下しています。これを機能カテゴリごとにファイルを分割し、テストの実行と保守を容易にします。

### 分割方針とターゲットファイル
既存の `tests/integration_test.rs` を以下のカテゴリ別ファイルに分割します：

1. **`tests/core_test.rs`**
   - 対象: 最小アセンブル、複数命令の連続、基本的なパス遷移や全体の流れに関係するコア動作テスト。
2. **`tests/macro_test.rs`**
   - 対象: `.macro`, `.rept`, `.irp`, `.irpc` などのマクロ展開機能や引数処理のテスト。
3. **`tests/pseudo_test.rs`**
   - 対象: `.dc`, `.ds`, `.comm`, `.offsym`, `.org` などの疑似命令、データ配置、セクション定義のテスト。
4. **`tests/fpu_test.rs`**
   - 対象: `fnop`, `fmove`, `fmovem`, `fsincos`, `fbcc`, `fdbcc`, `.fpid` などのFPU命令エンコードテスト。
5. **`tests/options_test.rs`**
   - 対象: 各種コマンドラインオプション（警告レベル、出力指定等）の挙動検証テスト。
6. **`tests/scd_test.rs`**
   - 対象: `-g` 指定時のSCDデバッグ拡張レコード出力や、`.file`, `.dim`, `.def` などのSCD用疑似命令の出力テスト。

### 実行手順
1. `tests/` ディレクトリ配下にターゲットファイルを新規作成する。
2. `tests/integration_test.rs` から対象のテスト関数群を順次切り出し、対応するファイルに移植する。
3. 移植ごとに `cargo test` を実行し、既存テストのパス（トータル98件の成功）を確認する。
4. すべてのテストが分割された後、空になった `tests/integration_test.rs` を削除する。

---

## 🔍 フェーズ2: 未使用コード（Clippy警告）の精密調査・クリーンアップ

### 目的
`cargo clippy` が指摘する未使用コード（デッドコード警告）を整理し、コードベースをクリーンにします。ただし、単なる削除ではなく、オリジナルとの比較とコミット履歴の追跡を行い、**移植時の実装漏れ（バグ）**がないかを厳密に調査します。

### 調査・分析のプロセス
すべての候補に対し、以下の手順で分析と意思決定を行います：

1. **存在理由の特定**: コードの用途やコメントから、将来的な機能のために用意されたものか、一時的な実装残りかを把握。
2. **オリジナルアセンブラ（HAS060）の調査**:
   - `external/has060xx/src/` 配下のソースコード（`symbol.equ`, `cputype.equ`, `work.s` 等）を調査。
   - 「オリジナルでも未使用定義だったのか」「オリジナルでは使用されていたのか」を特定。
3. **コミット履歴の追跡と移植漏れ検証**:
   - `git log -S <symbol>` や `git blame` を実行し、過去にそのコードが使われていた時期があったか、あるいはどの時点で追加されて放置されたかを追跡。
   - 「本来実装すべきロジックが未実装なために、結果的に未使用になっている（移植漏れバグ）」ケースを発見した場合、削除ではなく**本来の実装を記述・修正**する。
4. **アクションの決定**:
   - **削除**: オリジナルでも不要、またはRust移植版で不要と確認されたコード。
   - **バグ修正**: 移植漏れ・実装ミスの修正。
   - **サプレッション**: 仕様上の整合性や将来の拡張性のために残す必要がある場合、`#[allow(dead_code)]` を付与し、その理由をコメントで明記。

### 対象候補リストと調査メモ
Clippyで警告された以下の159個の警告（重複を除く約25種類の項目）について調査を行います：

| 分類 | 警告対象コード | 所在ファイル | 調査観点・オリジナル対応ファイル候補 |
| :--- | :--- | :--- | :--- |
| **関数/メソッド** | `is_visibility_directive` | `src/pass/pseudo/misc.rs` | オリジナル pseudo.s での visibility ディレクティブ処理漏れ確認 |
| | `SourceBuf::from_bytes` | `src/source/buf.rs` | 単体テスト用やバッファ生成用のユーティリティが未使用か確認 |
| | `SymbolTable::is_defined` | `src/symbol/mod.rs` | シンボル定義確認ロジックが他のパスで使われるべきだったか |
| | `SymbolTable::user_sym_count` | `src/symbol/mod.rs` | 統計情報出力機能 (`-l`等) やデバッグ表示での参照漏れ確認 |
| | `SymbolTable::cmd_count` | `src/symbol/mod.rs` | 同上 |
| | `SymbolTable::reg_count` | `src/symbol/mod.rs` | 同上 |
| | `Symbol::is_builtin` | `src/symbol/types.rs` | 予約語や命令名の判定ロジックが未使用か確認 |
| | `Symbol::is_pseudo` | `src/symbol/types.rs` | 疑似命令判定ロジックが未使用か確認 |
| | `Symbol::is_local` | `src/symbol/types.rs` | ローカルシンボルの判定が特定のパスで必要だったか確認 |
| | `SizeFlags::contains` | `src/symbol/types.rs` | サイズフラグの包含判定が命令サイズ選択で使われるべきか |
| **enumバリアント** | `SizeCode::Quad` | `src/symbol/types.rs` | `.q` (MMU命令) 対応。オリジナルでのMMU機能有無と移植状況確認 |
| | `DefAttrib::Predefine` | `src/symbol/types.rs` | 予約シンボル定義属性。`symbol.equ` の SA_PREDEFINE 対応 |
| | `ExtAttrib::Globl` | `src/symbol/types.rs` | グローバルシンボル属性。`symbol.equ` の $FA (Globl) 対応 |
| | `FirstDef::Set` | `src/symbol/types.rs` | `.set`(=) 定義状態。オリジナルでの挙動と移植状況 |
| | `Symbol::Real` | `src/symbol/types.rs` | 実数データ（FPU関連）のシンボル型。fexpr.s 等での扱い確認 |
| **構造体フィールド** | `Symbol::Value::org_num` | `src/symbol/types.rs` | オリジンの `org` 番号保持用。`.org` 処理での値確認 |
| | `Symbol::Value::opt_count` | `src/symbol/types.rs` | 分岐最適化の回数管理用。pass2.s での参照確認 |
| | `Symbol::Opcode::noopr` | `src/symbol/types.rs` | オペランドなし命令判定用。`opname.s` のフラグ移植状況 |
| | `Symbol::Opcode::size` | `src/symbol/types.rs` | 使用可能サイズ。命令デコード・エンコードでのチェック |
| | `Symbol::Opcode::size2` | `src/symbol/types.rs` | ColdFireで使用可能なサイズ。命令チェック漏れ確認 |
| | `Symbol::Macro::local_count`| `src/symbol/types.rs` | マクロ内ローカルラベルカウンタ。macro.s 参照 |
| **定数/マスク** | `SizeFlags::Q`, `SizeFlags::BW` | `src/symbol/types.rs` | サイズ指定マスク定数の未使用理由確認 |
| | `CpuMask::NONE`, `CpuMask::CF`, `CpuMask::ALL` | `src/symbol/types.rs` | CPU種別判定マスク。cputype.equ / main.s 参照 |
| | MMUレジスタ定数群 (`CRP`〜`BC`) | `src/symbol/types.rs` | MMU制御レジスタコード。MMU命令パーサでの実装有無の確認 |

---

## 🛠️ フェーズ3: エラー・警告処理の統一・抽象化

### 目的
現在、エラー出力や警告メッセージが `std::io::stderr()` への直接書き込みと各コンテキストでのカウント管理で混在しています。これらを抽象的な `ErrorReporter` トレイトに統合し、出力先を差し替え可能にすることで、テスト容易性の向上とアーキテクチャのクリーン化を実現します。

### 具体的な設計と実装計画

#### 1. `ErrorReporter` トレイトの定義 (`src/error/reporter.rs`)
`src/error/reporter.rs` を新規作成し、以下のインターフェースを定義します。
```rust
use crate::error::{ErrorCode, WarnCode, SourcePos};

pub trait ErrorReporter {
    /// エラーを報告する
    fn report_error(&mut self, pos: &SourcePos, code: ErrorCode, symbol: Option<&[u8]>);
    
    /// 警告を報告する
    fn report_warning(&mut self, pos: &SourcePos, code: WarnCode, symbol: Option<&[u8]>);
    
    /// 発生したエラーの総数
    fn error_count(&self) -> u32;
    
    /// 発生した警告の総数
    fn warning_count(&self) -> u32;
    
    /// 状態をクリアする（複数ファイル処理用）
    fn reset(&mut self);
}
```

#### 2. レポーターの実装
- **`StderrReporter`**:
  - `main.rs` で使用する標準エラー出力用レポーター。
  - 生成時に `warn_level` (u8) を受け取り、設定レベル未満の警告は出力せず、かつカウントも行わない。
- **`BufferReporter`**:
  - テストコード等で使用する、メモリ蓄積用レポーター。
  - 発生したエラー/警告を構造体の `Vec` に保存し、テスト内で「特定のエラーが正しく検出されたか」をアサーションできるようにする。

#### 3. コンテキストおよびパス処理への統合
- `pass::assemble` のシグネチャを以下のように更新します：
  ```rust
  pub fn assemble(
      ctx: &mut AssemblyContext,
      reporter: &mut dyn ErrorReporter,
  ) -> Result<AssembleResult, AssembleError>
  ```
- 各アセンブリパス `pass1::pass1`、および `pass3::pass3` に `reporter` への参照を渡します。
- `P1Ctx` / `P3Ctx` に保持されている `std::io::stderr()` への直接書き込みロジックを、すべて `reporter.report_error` / `report_warning` の呼び出しに置換します。
- `AssemblyContext` に存在する `num_errors` と `num_warnings` フィールドを廃止、または `ErrorReporter` からの集計値に同期する設計へ統合します。

#### 4. テスト環境の移行とクリーン化
- テストコード（`tests/integration_test.rs` から分割された各テストファイル）において、`BufferReporter` を用いてアセンブル中のエラーを検証します。
- テスト実行時の stderr 汚染がなくなり、CI等のテストログがクリーンになります。
