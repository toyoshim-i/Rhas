/// Pass 3: オブジェクトコード生成
///
/// TempRecord 列とシンボルテーブルから最終的なバイト列と
/// 外部シンボル情報を生成し、ObjectCode を返す。

use crate::addressing::EffectiveAddress;
use crate::expr::{eval_rpn, Rpn};
use crate::expr::eval::EvalValue;
use crate::instructions::encode_insn;
use crate::object::{ExternalSymbol, ObjectCode, SectionInfo, SymKind};
use crate::symbol::{Symbol, SymbolTable};
use crate::symbol::types::{DefAttrib, ExtAttrib, InsnHandler, SizeCode};
use super::prn::PrnLine;
use super::temp::{branch_word_size, TempRecord};

// ----------------------------------------------------------------
// Pass3 エラー
// ----------------------------------------------------------------

#[derive(Debug)]
pub enum Pass3Error {
    UndefinedSymbol(Vec<u8>),
    BranchOutOfRange { offset: i32 },
    Encoding,
}

// ----------------------------------------------------------------
// Pass3 内部状態
// ----------------------------------------------------------------

struct P3Ctx<'a> {
    sym: &'a SymbolTable,
    /// 現在のセクション ID（1=text, 2=data, ...）
    cur_sect: u8,
    /// 各セクションのバイト列
    sect_bytes: [Vec<u8>; 10],
    /// 各セクションの BSS サイズ（sect_bytes を持たないセクション）
    sect_bss_size: [u32; 10],
    /// 各セクションのロケーションカウンタ
    loc_ctr: [u32; 10],
    /// 行頭ロケーション（'*' 用）
    loc_top: u32,
    /// 外部シンボル
    ext_syms: Vec<ExternalSymbol>,
    /// エラー数
    num_errors: u32,
    // ---- HLK コードボディ生成（20xx/10xx 形式）----
    /// 構築済みコードボディ（20xx セクション切り替え + 10xx ブロック）
    code_body: Vec<u8>,
    /// 現在フラッシュ待ちのバイト（次のセクション切り替えか終了時にフラッシュ）
    code_buf: Vec<u8>,
    // ---- PRNリスト生成 ----
    /// PRN生成が有効か
    prn_enable: bool,
    /// 現在の行の情報（line_num, start_loc, start_sect, text, is_macro, accumulated_bytes）
    prn_pending: Option<(u32, u32, u8, Vec<u8>, bool, Vec<u8>)>,
    /// 収集済みのPRN行リスト
    pub prn_lines: Vec<PrnLine>,
}

impl<'a> P3Ctx<'a> {
    fn new(sym: &'a SymbolTable, prn_enable: bool) -> Self {
        P3Ctx {
            sym,
            cur_sect: 1,
            sect_bytes: Default::default(),
            sect_bss_size: [0u32; 10],
            loc_ctr: [0u32; 10],
            loc_top: 0,
            ext_syms: Vec::new(),
            num_errors: 0,
            code_body: Vec::new(),
            code_buf: Vec::new(),
            prn_enable,
            prn_pending: None,
            prn_lines: Vec::new(),
        }
    }

    /// code_buf を 10xx ブロックとして code_body にフラッシュする
    fn flush_code_buf(&mut self) {
        if self.code_buf.is_empty() { return; }
        let buf = std::mem::take(&mut self.code_buf);
        let mut i = 0;
        while i < buf.len() {
            let chunk = (buf.len() - i).min(256);
            self.code_body.push(0x10);
            self.code_body.push((chunk - 1) as u8);
            // ワード単位で書き出し（奇数バイトは 00 でパディング）
            let mut j = i;
            while j < i + chunk {
                self.code_body.push(buf[j]);
                self.code_body.push(if j + 1 < i + chunk { buf[j + 1] } else { 0 });
                j += 2;
            }
            i += chunk;
        }
    }

    fn sect_idx(&self) -> usize {
        (self.cur_sect as usize).saturating_sub(1).min(9)
    }

    fn location(&self) -> u32 {
        self.loc_ctr[self.sect_idx()]
    }

    fn advance(&mut self, n: u32) {
        let idx = self.sect_idx();
        self.loc_ctr[idx] = self.loc_ctr[idx].wrapping_add(n);
    }

    fn emit(&mut self, bytes: &[u8]) {
        let idx = self.sect_idx();
        // BSS/Stack セクション (id >= 3) の場合はサイズのみ記録
        let bss_like = self.cur_sect >= 3;
        if bss_like {
            self.sect_bss_size[idx] += bytes.len() as u32;
        } else {
            self.sect_bytes[idx].extend_from_slice(bytes);
            self.code_buf.extend_from_slice(bytes);
            // PRNバイト追跡
            if self.prn_enable {
                if let Some(ref mut pending) = self.prn_pending {
                    pending.5.extend_from_slice(bytes);
                }
            }
        }
        self.advance(bytes.len() as u32);
    }

    fn emit_zeros(&mut self, count: u32) {
        let idx = self.sect_idx();
        let bss_like = self.cur_sect >= 3;
        if bss_like {
            self.sect_bss_size[idx] += count;
        } else {
            let zeros: Vec<u8> = vec![0u8; count as usize];
            self.sect_bytes[idx].extend_from_slice(&zeros);
            self.code_buf.extend_from_slice(&zeros);
        }
        self.advance(count);
    }

    /// 現在のPRNペンディング行をprn_linesにフラッシュする
    fn prn_flush(&mut self) {
        if let Some((line_num, loc, sect, text, is_macro, bytes)) = self.prn_pending.take() {
            self.prn_lines.push(PrnLine {
                line_num, location: loc, section: sect, bytes, text, is_macro
            });
        }
    }

    /// PRNペンディングを開始する
    fn prn_start(&mut self, line_num: u32, text: Vec<u8>, is_macro: bool) {
        if self.prn_enable {
            self.prn_flush();
            let loc = self.location();
            let sect = self.cur_sect;
            self.prn_pending = Some((line_num, loc, sect, text, is_macro, Vec::new()));
        }
    }

    /// RPN 式を評価する
    fn eval(&self, rpn: &Rpn) -> Result<EvalValue, Vec<u8>> {
        let loc = self.loc_top;
        let cur = self.location();
        let sec = self.cur_sect;
        let sym = self.sym;
        eval_rpn(rpn, loc, cur, sec, &|name| {
            sym.lookup_sym(name).and_then(sym_to_eval)
        }).map_err(|e| {
            match e {
                crate::expr::eval::EvalError::UndefinedSymbol(n) => n,
                _ => b"<eval error>".to_vec(),
            }
        })
    }
}

fn sym_to_eval(sym: &Symbol) -> Option<EvalValue> {
    if let Symbol::Value { value, section, attrib, .. } = sym {
        if *attrib >= DefAttrib::NoDet {
            return Some(EvalValue { value: *value, section: *section });
        }
    }
    None
}

// ----------------------------------------------------------------
// Pass3 メイン
// ----------------------------------------------------------------

/// Pass3: TempRecord → ObjectCode + PRN行リスト
pub fn pass3(
    records:     &[TempRecord],
    sym:         &SymbolTable,
    source_name: Vec<u8>,
    prn_enable:  bool,
    max_align:   u8,
) -> (ObjectCode, Vec<PrnLine>) {
    let mut ctx = P3Ctx::new(sym, prn_enable);
    let mut obj = ObjectCode::new(source_name);
    if max_align > 0 {
        obj.has_align = true;
        obj.max_align = max_align;
    }

    for rec in records {
        ctx.loc_top = ctx.location();

        match rec {
            TempRecord::Const(bytes) => {
                ctx.emit(bytes);
            }

            TempRecord::DeferredInsn { base, handler, size, ops, .. } => {
                process_deferred(&mut ctx, *base, *handler, *size, ops);
            }

            TempRecord::Branch { opcode, target, req_size } => {
                process_branch(&mut ctx, *opcode, target, *req_size);
            }

            TempRecord::Data { size, rpn } => {
                match ctx.eval(rpn) {
                    Ok(v) => {
                        let bytes = val_to_bytes(v.value, *size);
                        ctx.emit(&bytes);
                    }
                    Err(_) => {
                        // 外部参照が必要 → ゼロで埋めてリロケーション記録（省略）
                        ctx.emit_zeros(*size as u32);
                        ctx.num_errors += 1;
                    }
                }
            }

            TempRecord::Ds { byte_count } => {
                ctx.emit_zeros(*byte_count);
            }

            TempRecord::Align { n, pad, section } => {
                let align = 1u32 << *n;
                let loc = ctx.location();
                let new_loc = (loc + align - 1) & !(align - 1);
                let pad_bytes = new_loc - loc;
                let _ = section;
                if pad_bytes > 0 {
                    // パディング値で埋める
                    let p = *pad;
                    let mut fill = Vec::new();
                    let mut remaining = pad_bytes;
                    while remaining >= 2 {
                        fill.push((p >> 8) as u8);
                        fill.push(p as u8);
                        remaining -= 2;
                    }
                    if remaining == 1 { fill.push(p as u8); }
                    ctx.emit(&fill);
                }
            }

            TempRecord::LabelDef { .. } => {
                // シンボル値は Pass2 で確定済み → 何もしない
            }

            TempRecord::SectChange { id } => {
                // code_buf をフラッシュしてからセクション切り替え
                ctx.flush_code_buf();
                ctx.cur_sect = *id;
                // BSS/Stack 以外（コードを持つセクション）の場合 20xx レコードを出力
                if *id <= 2 || (*id >= 5 && *id != 6 && *id != 7 && *id != 9 && *id != 10) {
                    ctx.code_body.push(0x20);
                    ctx.code_body.push(*id);
                    ctx.code_body.extend_from_slice(&[0, 0, 0, 0]);
                }
            }

            TempRecord::Org { value } => {
                let idx = ctx.sect_idx();
                ctx.loc_ctr[idx] = *value;
            }

            TempRecord::XDef { name } => {
                // シンボルテーブルから値を取得して外部定義として記録
                let (val, kind) = if let Some(s) = sym.lookup_sym(name) {
                    if let Symbol::Value { value, ext_attrib, .. } = s {
                        let k = match ext_attrib {
                            ExtAttrib::Globl => SymKind::Globl,
                            _ => SymKind::XDef,
                        };
                        (*value as u32, k)
                    } else { (0, SymKind::XDef) }
                } else { (0, SymKind::XDef) };
                ctx.ext_syms.push(ExternalSymbol {
                    kind, value: val, name: name.clone()
                });
            }

            TempRecord::XRef { name } => {
                // 外部参照シンボル番号を割り当て（1から連番）
                let num = ctx.ext_syms.len() as u32 + 1;
                ctx.ext_syms.push(ExternalSymbol {
                    kind: SymKind::XRef, value: num, name: name.clone()
                });
            }

            TempRecord::Globl { name } => {
                let (val, kind) = if let Some(s) = sym.lookup_sym(name) {
                    if let Symbol::Value { value, ext_attrib, attrib, .. } = s {
                        if *attrib >= DefAttrib::Define {
                            ((*value) as u32, SymKind::Globl)
                        } else {
                            // 未定義 → XRef
                            let num = ctx.ext_syms.len() as u32 + 1;
                            (num, SymKind::XRef)
                        }
                    } else { (0, SymKind::Globl) }
                } else {
                    let num = ctx.ext_syms.len() as u32 + 1;
                    (num, SymKind::XRef)
                };
                ctx.ext_syms.push(ExternalSymbol {
                    kind, value: val, name: name.clone()
                });
            }

            TempRecord::End => {
                break;
            }

            TempRecord::Cpu { .. } => {
                // CPU 変更は Pass1/Pass2 で処理済み
            }

            TempRecord::LineInfo { line_num, text, is_macro } => {
                ctx.prn_start(*line_num, text.clone(), *is_macro);
            }
        }
    }

    // 最後のPRN行をフラッシュ
    if ctx.prn_enable {
        ctx.prn_flush();
    }

    // 残りの code_buf をフラッシュ
    ctx.flush_code_buf();
    obj.code_body = std::mem::take(&mut ctx.code_body);

    let prn_lines = ctx.prn_lines;

    // セクション情報を構築（常に text/data/bss/stack の4セクションを出力）
    for sect_id in 1u8..=4 {
        let idx = (sect_id as usize) - 1;
        let bytes = &ctx.sect_bytes[idx];
        let size = if sect_id >= 3 {
            // BSS 系 = ロケーションカウンタの値
            ctx.loc_ctr[idx]
        } else {
            bytes.len() as u32
        };
        obj.sections.push(SectionInfo {
            id:    sect_id,
            bytes: if sect_id >= 3 { Vec::new() } else { bytes.clone() },
            size,
        });
    }
    // 相対セクション（5〜10）は使用時のみ追加
    for sect_id in 5u8..=10 {
        let idx = (sect_id as usize) - 1;
        let bytes = &ctx.sect_bytes[idx];
        let size = if sect_id == 6 || sect_id == 7 || sect_id == 9 || sect_id == 10 {
            ctx.loc_ctr[idx]
        } else {
            bytes.len() as u32
        };
        if size > 0 || !bytes.is_empty() {
            obj.sections.push(SectionInfo {
                id:    sect_id,
                bytes: if sect_id == 6 || sect_id == 7 || sect_id == 9 || sect_id == 10 {
                    Vec::new()
                } else {
                    bytes.clone()
                },
                size,
            });
        }
    }

    obj.ext_syms = ctx.ext_syms;
    (obj, prn_lines)
}

// ----------------------------------------------------------------
// 未解決命令の処理
// ----------------------------------------------------------------

fn process_deferred(
    ctx:     &mut P3Ctx<'_>,
    base:    u16,
    handler: InsnHandler,
    size:    SizeCode,
    ops:     &[EffectiveAddress],
) {
    // EA 内の RPN を評価した新しい EA を構築
    let resolved: Vec<EffectiveAddress> = ops.iter()
        .map(|ea| resolve_ea(ctx, ea))
        .collect();

    match encode_insn(base, handler, size, &resolved) {
        Ok(bytes) => ctx.emit(&bytes),
        Err(_) => {
            // 未解決のまま → ゼロバイトで埋める
            // 正確なサイズは推定値から引き継ぐ
            let est = 2 + resolved.iter().map(|ea| ea_ext_size(ea)).sum::<u32>();
            ctx.emit_zeros(est);
            ctx.num_errors += 1;
        }
    }
}

/// EA 内の RPN 式を評価して定数 EA を返す
fn resolve_ea(ctx: &P3Ctx<'_>, ea: &EffectiveAddress) -> EffectiveAddress {
    use crate::expr::rpn::RPNToken;
    let resolved_rpn = |rpn: &Rpn| -> Rpn {
        match ctx.eval(rpn) {
            Ok(v) => vec![RPNToken::Value(v.value as u32), RPNToken::End],
            Err(_) => rpn.clone(),
        }
    };
    match ea {
        EffectiveAddress::Immediate(rpn) =>
            EffectiveAddress::Immediate(resolved_rpn(rpn)),
        EffectiveAddress::AbsShort(rpn) =>
            EffectiveAddress::AbsShort(resolved_rpn(rpn)),
        EffectiveAddress::AbsLong(rpn) =>
            EffectiveAddress::AbsLong(resolved_rpn(rpn)),
        other => other.clone(),
    }
}

fn ea_ext_size(ea: &EffectiveAddress) -> u32 {
    match ea {
        EffectiveAddress::DataReg(_) | EffectiveAddress::AddrReg(_)
        | EffectiveAddress::AddrRegInd(_) | EffectiveAddress::AddrRegPostInc(_)
        | EffectiveAddress::AddrRegPreDec(_) => 0,
        EffectiveAddress::AbsShort(_) | EffectiveAddress::AddrRegDisp { .. }
        | EffectiveAddress::PcDisp(_) => 2,
        EffectiveAddress::AbsLong(_) => 4,
        EffectiveAddress::Immediate(_) => 2,
        EffectiveAddress::AddrRegIdx { .. } | EffectiveAddress::PcIdx { .. } => 2,
    }
}

// ----------------------------------------------------------------
// 分岐命令の処理
// ----------------------------------------------------------------

fn process_branch(
    ctx:      &mut P3Ctx<'_>,
    opcode:   u16,
    target:   &Rpn,
    req_size: Option<SizeCode>,
) {
    let pc = ctx.location(); // 命令先頭のアドレス

    // ターゲットアドレスを評価
    let target_addr = match ctx.eval(target) {
        Ok(v) => v.value,
        Err(_) => {
            // 未解決 → ゼロオフセットで出力
            let sz = branch_word_size(req_size);
            ctx.emit_zeros(sz);
            return;
        }
    };

    // オフセット計算 (target - (pc + 2))
    let offset_w = target_addr - (pc as i32 + 2);

    match req_size {
        Some(SizeCode::Short) => {
            // .s 形式: 2バイト (オフセットを下位バイトに埋め込む)
            if offset_w >= -128 && offset_w <= 127 {
                let b1 = (opcode | (offset_w as u16 & 0xFF)) as u8;
                let b0 = (opcode >> 8) as u8;
                ctx.emit(&[b0, b1]);
            } else {
                ctx.emit_zeros(2);
                ctx.num_errors += 1;
            }
        }
        Some(SizeCode::Long) => {
            // .l 形式: 6バイト
            // オペコード = $xxFF, その後32bitオフセット
            let mut bytes = Vec::with_capacity(6);
            bytes.push((opcode >> 8) as u8);
            bytes.push(0xFF); // long form indicator
            let off_long = target_addr - (pc as i32 + 2 + 4);
            bytes.push((off_long >> 24) as u8);
            bytes.push((off_long >> 16) as u8);
            bytes.push((off_long >> 8) as u8);
            bytes.push(off_long as u8);
            ctx.emit(&bytes);
        }
        _ => {
            // .w 形式 (デフォルト): 4バイト
            if offset_w >= -32768 && offset_w <= 32767 {
                let w = offset_w as i16 as u16;
                let b0 = (opcode >> 8) as u8;
                let b1 = opcode as u8; // 下位バイトは 0x00 for .w
                ctx.emit(&[b0, b1, (w >> 8) as u8, w as u8]);
            } else {
                ctx.emit_zeros(4);
                ctx.num_errors += 1;
            }
        }
    }
}

// ----------------------------------------------------------------
// ユーティリティ
// ----------------------------------------------------------------

fn val_to_bytes(v: i32, size: u8) -> Vec<u8> {
    match size {
        1 => vec![v as u8],
        2 => { let w = v as u16; vec![(w >> 8) as u8, w as u8] }
        4 => {
            let l = v as u32;
            vec![(l>>24) as u8, (l>>16) as u8, (l>>8) as u8, l as u8]
        }
        _ => vec![],
    }
}
