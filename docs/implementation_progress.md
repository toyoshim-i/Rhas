# Rhas 実装進捗

## 現在の状態（2026-03-01）
- ベース: HAS060X.X v1.2.5 / HAS v3.09+91
- MS5: 達成（比較対象 17/17 一致）
- MS6: 達成（FPU/SCD/残互換機能を実装）

## マイルストーン
| MS | 目標 | 状態 |
|---|---|---|
| MS1 | 最小命令とオブジェクト出力 | ✅ |
| MS2 | 68000 全整数命令 + 参照解決 | ✅ |
| MS3 | 疑似命令・最適化で `HANOI.S` 通過 | ✅ |
| MS4 | マクロ処理で `K_MACRO.MAC` 通過 | ✅ |
| MS5 | 実ソース比較で完全一致 | ✅ |
| MS6 | FPU/SCD/残互換機能の完了 | ✅ |

## 実装済み（要約）
### コア
- CLI/オプション互換
- 3パス（Pass1/2/3）
- HLK オブジェクト出力
- シンボル/式/RPN/EA/命令エンコード

### 命令・疑似命令
- 68000 基本命令: 完了
- 68010/020/040 拡張・ColdFire 主要拡張: 完了
- 疑似命令・条件分岐・マクロ: 完了
- PRN / SYM 出力: 完了

### MS6 関連（完了済み部分）
- `.offsym` / `.fpid`
- FPU コア命令（`fnop/fsave/frestore/fmove/fmovecr/fadd/fsub/fmul/fdiv/fcmp/ftst`）
- `fsincos`（`FPn`/EA ソース + `FPc:FPs` 宛先）
- FPU レジスタオペランド（`FPn/FPCR/FPSR/FPIAR`）と CPID 反映
- FMOVEM（制御レジスタ単体/複合指定 + FPn 静的/動的レジスタリスト）
- FPU 分岐系（`fbcc` / `fdbcc` 全条件バリアント）
- SCD の HAS 互換2モード（`-g` と `.file` モード）
- SCD フッタ（可変長エントリ、`.file` 長名/`SCDFILENUM`、`next` チェイン、`.val` 再評価）

## テスト状況
| スイート | 状態 |
|---|---|
| ユニット | ✅ 全通過 |
| 統合（97） | ✅ 全通過 |
| ゴールデン（25） | ✅ 全通過 |
| エラーメッセージ比較（9） | ✅ 全通過 |
| MS5簡易比較（17） | ✅ 全一致 |
| MS6拡張比較（19） | ✅ 全一致 |

## 既知の互換ギャップ
全解消。ColdFire CPU 選択 / `.cpu` 数式パラメータ / FBcc・FDBcc 外部参照 / Bcc.L・FBcc.L RPN リロケーションの4件を対応済み。

## 検証残タスク
- 互換ギャップ・検証テスト拡充は全完了。
- 残りはクリーンアップ系（優先度C）のみ。詳細は [verification_backlog.md](verification_backlog.md) を参照。

## 直近コミット（ドキュメント時点）
- `1d75cb2` Support Bcc.L and FBcc.L external reference with RPN relocation
- `98fb1b3` Support FBcc/FDBcc external reference targets with PC-relative relocation
- `1ca0181` Connect .cpu directive to evaluate expression and set CPU type
- `c7f715a` Connect ColdFire CPU selection directives (.5200/.5300/.5400) in pass1
- `c161860` Document remaining compatibility gaps for full HAS interop
- `e8f8f02` Achieve zero warnings: remove dead helpers and suppress spec tables
- `65a6f0e` Eliminate all inline error() calls in pass1, route through error table

## 参照
- [README](../README.md)
- [テストガイド](testing.md)
- [HLKフォーマット](hlk_object_format.md)
- [HASアーキテクチャ](has_architecture.md)
