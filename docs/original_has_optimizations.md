# オリジナルHAS最適化ガイド（フラグ対応一覧付き）

このドキュメントは、`external/has060xx/src` に含まれる **オリジナル HAS060X.X v1.2.5** の実装を読み解き、最適化処理を網羅的に整理したものです。  
対象ソースは主に `main.s`（オプション解釈）, `doasm.s`（命令組み立て時最適化）, `optimize.s`（Pass2最適化）, `objgen.s`（オブジェクト出力時変換）, `work.s`（フラグ定義）です。

## 1. 全体像

HAS の最適化は大きく3層です。

1. Pass1（`doasm.s`）  
命令を組み立てる時点でできる局所最適化（即値・命令置換）。
2. Pass2（`optimize.s`）  
前方参照解決後にサイズ再評価を繰り返す最適化（分岐・変位・命令削除）。
3. Pass3/Object生成（`objgen.s`）  
Pass2で付与された情報を使った最終変換（例: 直後 `BSR` の `PEA` 化）。

## 2. フラグ体系（最適化に関係するもの）

### 2.1 `-c[n]` の意味

| 指定 | 主効果 |
|---|---|
| `-c0` | 最適化全停止に近い設定。前方参照最適化禁止、`NOQUICK`/`NONULDISP`/`NOBRACUT` 有効、拡張最適化OFF。 |
| `-c1` | `NONULDISP` のみ有効（`0(An)->(An)` の削除を禁止）。 |
| `-c2` | v2互換。`NONULDISP`/`NOBRACUT` 有効、拡張最適化OFF。`-a`/`-q` と連動。 |
| `-c3` | v3互換。v2互換を解除し、拡張最適化OFF。 |
| `-c4` | 拡張最適化（12種）を全ON。v2互換解除、前方参照最適化許可。 |
| `-c` | 数字省略時は `-c2` 扱い。 |

### 2.2 `-c<mnemonic>` 系

| 指定 | 効果 |
|---|---|
| `-cfscc[=6]` | `FScc -> FBcc` 展開を有効。`=6` 付きは 68060 限定。 |
| `-cmovep[=6]` | `MOVEP -> MOVE` 展開を有効。`=6` 付きは 68060 限定。 |
| `-call[=6]` | 上2つを同時有効。 |

### 2.3 その他の関連フラグ

| 指定 | 効果 |
|---|---|
| `-n` | Pass2前方参照最適化を禁止（`OPTIMIZE=1`）。 |
| `-q` | v2互換時（`-c2`）に `NOQUICK` を有効化し、Quick変換を禁止。 |
| `-a` | v2互換時（`-c2`）に絶対ショート変換禁止を有効化。 |
| `-b[n]` | PC相対と絶対ロング、`BRA/BSR/Bcc` と `JBRA/JBSR/JBcc` の変換方針を変更。 |
| `-1` | 絶対ロングを optional PC間接へ変換（`-e` と `-b1` を伴う）。 |

## 3. 最適化一覧（どのフラグで有効か）

## 3.1 Pass1の局所最適化（`doasm.s`）

### A. `NOQUICK` 依存（`-c0` または `-c2 -q` で無効化）

| 最適化 | 概要 | 有効条件 |
|---|---|---|
| `ADD/SUB #1..8,<ea> -> ADDQ/SUBQ` | `subadd` 系で即値をQuick化 | `NOQUICK=0` |
| `MOVE.L #(-128..127),Dn -> MOVEQ` | ロング即値を `MOVEQ` 化 | `NOQUICK=0` |

### B. `-c4` 拡張最適化フラグ群（12種）

| 内部フラグ | 変換内容（代表） | 有効化 |
|---|---|---|
| `OPTCLR` | `CLR.L Dn -> MOVEQ #0,Dn`（68000/010向け） | `-c4` |
| `OPTMOVEA` | `MOVEA.L #imm16,An -> MOVEA.W #imm,An` / `MOVEA.L An,An` 削除 / `MOVEA #0,An -> SUBA.L An,An` | `-c4` |
| `OPTADDASUBA` | `ADDA/SUBA #imm,An` を `ADDQ/SUBQ` または `LEA` 化、特例 `...#8000...` 変換あり | `-c4` |
| `OPTCMPA` | `CMPA #0,An -> TST.L An`（68020+）/ `CMPA.L #imm16,An -> CMPA.W #imm,An` | `-c4` |
| `OPTLEA` | `LEA (An),An`/`LEA (0,An),An` 削除、`LEA (±1..8,An),An -> ADDQ/SUBQ`、`LEA 0,An -> SUBA.L An,An` | `-c4` |
| `OPTASL` | `ASL #1,Dn -> ADD Dn,Dn`（68060除外） | `-c4` |
| `OPTCMP0` | `CMP #0,Dn -> TST Dn` | `-c4` |
| `OPTMOVE0` | `MOVE.B/W #0,Dn -> CLR.B/W Dn` | `-c4` |
| `OPTCMPI0` | `CMPI #0,<ea> -> TST <ea>` | `-c4` |
| `OPTSUBADDI0` | `ADDI/SUBI #1..8,<ea> -> ADDQ/SUBQ` | `-c4` |
| `OPTBSR` | 直後 `BSR` を `PEA (2,PC)` 化（最終出力時） | `-c4` |
| `OPTJMPJSR` | `JMP/JSR label` を `JBRA/JBSR` 系へ、`JMP (2,PC)` 削除、`JSR (2,PC) -> PEA (2,PC)` | `-c4` |

## 3.2 Pass2のサイズ最適化（`optimize.s`）

`-n` または `-c0` で `OPTIMIZE=1` の場合、Pass2最適化は実行されません。

| 最適化対象 | 主な変換 | 関連フラグ |
|---|---|---|
| `(d,An)` | `.w <-> .l` 切替、`d=0` のとき `(An)` へサプレス | `NONULDISP`（1ならサプレス禁止） |
| `(d,PC)` / `(d,OPC)` | 相対値再計算により `.w <-> .l` | `OPTIMIZE` |
| `(d,An,Rn)` / `(d,PC,Rn)` | `.s/.w/.l` の再選択 | `OPTIMIZE` |
| `(bd,An)` / `(bd,PC)` / `od` | 拡張ワード種別と変位サイズを再選択、0サプレス | `NONULDISP`, `OPTIMIZE` |
| `LINK.W/L` | 即値が収まる場合にサイズ再選択 | `OPTIMIZE` |
| `BRA/BSR/Bcc` | `.l/.w/.s/削除(0)` を再判定 | `NOBRACUT`（1なら直後分岐削除禁止） |
| `JBRA/JBSR/JBcc` | `BRA` 系同様にサイズ再判定 | `OPTIMIZE` |
| `CPBcc` | `.w/.l/削除` を再判定 | `NOBRACUT`, `OPTIMIZE` |
| `T_NOPDEATH` | 不要NOPの除去 | `OPTIMIZE`（およびエラッタ処理生成物） |

補足:

- Pass2は「収束するまで反復」します（`OPTFLAG` が立つ限りループ）。
- 初回は再拡大可能な `ESZ_OPT` を残し、次周回以降で確定させる作りです。

## 3.3 Pass3/Object生成時の最終変換（`objgen.s`）

| 最適化 | 内容 | 有効条件 |
|---|---|---|
| 直後 `BSR.W` の `PEA (2,PC)` 化 | Pass2で直後分岐マーク（`$40`）が付いた `BSR` を置換 | `OPTBSR=1`（通常 `-c4`） |

注意:

- `align` パディングを跨ぐ「見かけ上の直後分岐」は `PEA` にせず `BSR.W` を維持します（実行順序保護）。

## 4. フラグ別の有効化早見表

| 最適化カテゴリ | `-c0` | `-c1` | `-c2` | `-c3` | `-c4` | `-n` |
|---|---:|---:|---:|---:|---:|---:|
| Pass2 前方参照最適化 | × | ○ | ○ | ○ | ○ | × |
| Quick変換（ADDQ/MOVEQ等） | × | ○ | ○ (`-q`で×) | ○ | ○ | ○ |
| 0変位サプレス（`0(An)->(An)`） | × | × | × | ○ | ○ | ○ |
| 直後分岐削除（`BRA/Bcc -> 0`） | × | ○ | × | ○ | ○ | ○ |
| 拡張最適化12種（`OPT*`） | × | × | × | × | ○ | ○（`-c4`なら） |

`-n` は「拡張最適化をOFFにする」スイッチではなく、**Pass2の前方参照最適化だけを止める**点に注意してください。

## 5. 参照箇所（原典）

- `external/has060xx/src/main.s`  
  `option_c`, `option_b`, `option_1`, `option_n`, `option_c_x`
- `external/has060xx/src/doasm.s`  
  `~cmp`, `~subadd`, `~sbadcpa`, `~cmpi`, `~subaddi`, `~move`, `~movea`, `~clr`, `~jmpjsr`, `~lea`, `~asl`
- `external/has060xx/src/optimize.s`  
  `dispadr`, `disppc`, `dispopc`, `indexadr/indexpc`, `bdadr/bdpc/odisp`, `linkcmd`, `bracmd/jbracmd/cpbracmd`, `nopdeath`
- `external/has060xx/src/objgen.s`  
  `bracmd_w_test`, `bracmd_w_pea`
- `external/has060xx/src/work.s`  
  最適化フラグ定義（`OPT*`, `NOQUICK`, `NONULDISP`, `NOBRACUT` など）
