# Rhas 実装進捗

## 現在の状態（2026-03-01）
- ベース: HAS060X.X v1.2.5 / HAS v3.09+91
- MS5: 達成（比較対象 17/17 一致）
- MS6: 進行中（SCD は部分実装、FPU 未着手）

## マイルストーン
| MS | 目標 | 状態 |
|---|---|---|
| MS1 | 最小命令とオブジェクト出力 | ✅ |
| MS2 | 68000 全整数命令 + 参照解決 | ✅ |
| MS3 | 疑似命令・最適化で `HANOI.S` 通過 | ✅ |
| MS4 | マクロ処理で `K_MACRO.MAC` 通過 | ✅ |
| MS5 | 実ソース比較で完全一致 | ✅ |
| MS6 | FPU/SCD/残互換機能の完了 | 🚧 |

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
- `.offsym`（制約・上書き挙動含む）
- `.fpid`（範囲検証・無効化挙動）
- SCD 疑似命令の構文/値検証
- SCD TempRecord 化と Pass3 収集
- `$0000` 後の SCD フッタ出力（line/scd/exname）
- `func/.bf/.ef` 自動エントリ
- `.file` と `B204` の役割分離
- SCD 疑似命令の有効化条件（`.file` 必須）
- `-g` のみ時の SCD デフォルト行エントリ
- exname 条件を 14文字超へ調整
- `.type` のロング判定を HAS 互換（0x20/0x30 のみ）へ修正
- `.scl 16`（enum メンバ）の section を SCD 出力時に `-2` へ補正
- `.endef` の attrib 自動決定（function/tag/extern/static）を HAS 互換化
- `.ln/.line` の値域を HAS 互換化（下位16bitへ丸め）
- `.scl -1` 時の関数終端位置を保持し、SCD関数エントリ size へ反映
- `.tag` 参照を SCD `tag` フィールドへ反映
- `attrib` に応じた `next` チェイン（`.bb/.eb`, `.bf/.ef`, tag begin/end, function end）を反映

## テスト状況
| スイート | 状態 |
|---|---|
| ユニット | ✅ 全通過 |
| 統合（60） | ✅ 全通過 |
| ゴールデン（17） | ✅ 全通過 |
| MS5簡易比較（17） | ✅ 全一致 |

## MS6 残タスク（優先順）
1. FPU 命令（68881/68882）実装
2. SCD 出力の原典比較で未一致点を最終調整
3. 追加の互換疑似命令・境界挙動の詰め

## 直近コミット（ドキュメント時点）
- `3bb7f62` Align SCD `.file` exname threshold to 14+ chars
- `811d1e4` Emit HAS-style default SCD line entry for `-g` only
- `7274d6a` Match HAS SCD directive gating behind `.file`
- `c466dd0` Fix `.file` behavior split between SCD footer and B204

## 参照
- [README](../README.md)
- [テストガイド](testing.md)
- [HLKフォーマット](hlk_object_format.md)
- [HASアーキテクチャ](has_architecture.md)
