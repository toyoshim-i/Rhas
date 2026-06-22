//! Pass 3: オブジェクトコード生成
//!
//! TempRecord 列とシンボルテーブルから最終的なバイト列と
//! 外部シンボル情報を生成し、ObjectCode を返す。

use super::prn::PrnLine;
use super::temp::TempRecord;
use crate::expr::eval::EvalValue;
use crate::expr::rpn::RPNToken;
use crate::expr::{eval_rpn, Rpn};
use crate::object::{sym_kind, ExternalSymbol, ObjectCode, ScdEvent, SectionInfo};
use crate::symbol::types::{DefAttrib, ExtAttrib};
use crate::symbol::{Symbol, SymbolTable};

mod branch;
mod ea;
mod insn;

#[cfg(test)]
mod tests;

use branch::{process_branch, val_to_bytes};
use ea::{
    is_external_with_offset, is_simple_external, register_xdefs_in_ea, register_xdefs_in_rpn,
    resolve_regsym_chain, sym_to_eval,
};
use insn::process_deferred;

/// PRN pending info: (line_num, start_loc, start_sect, text, is_macro, accumulated_bytes)
type PrnPendingInfo = (u32, u32, u8, Vec<u8>, bool, Vec<u8>);

pub(super) struct P3Ctx<'a> {
    pub(super) sym: &'a SymbolTable,
    /// 現在のセクション ID（1=text, 2=data, ...）
    pub(super) cur_sect: u8,
    /// 各セクションのバイト列
    pub(super) sect_bytes: [Vec<u8>; 10],
    /// 現在のセクションでフラッシュ待ちの DSB サイズ（.ds in BSS/stack → $3000 レコードへ）
    pub(super) dsb_pending: u32,
    /// 各セクションのロケーションカウンタ
    pub(super) loc_ctr: [u32; 10],
    /// 行頭ロケーション（'*' 用）
    pub(super) loc_top: u32,
    /// 外部シンボル
    pub(super) ext_syms: Vec<ExternalSymbol>,
    /// エラー数
    pub(super) num_errors: u32,
    // ---- HLK コードボディ生成（20xx/10xx 形式）----
    /// 構築済みコードボディ（20xx セクション切り替え + 10xx ブロック）
    pub(super) code_body: Vec<u8>,
    /// 現在フラッシュ待ちのバイト（次のセクション切り替えか終了時にフラッシュ）
    pub(super) code_buf: Vec<u8>,
    // ---- PRNリスト生成 ----
    pub(super) prn_enable: bool,
    /// 現在の行の情報（line_num, start_loc, start_sect, text, is_macro, accumulated_bytes）
    pub(super) prn_pending: Option<PrnPendingInfo>,
    /// 収集済みのPRN行リスト
    pub prn_lines: Vec<PrnLine>,
    /// 現在のソース位置情報（エラー報告用）
    pub(super) current_pos: crate::error::SourcePos,
}

impl<'a> P3Ctx<'a> {
    pub(super) fn new(sym: &'a SymbolTable, prn_enable: bool, source_file: Vec<u8>) -> Self {
        P3Ctx {
            sym,
            cur_sect: 1,
            sect_bytes: Default::default(),
            dsb_pending: 0,
            loc_ctr: [0u32; 10],
            loc_top: 0,
            ext_syms: Vec::new(),
            num_errors: 0,
            code_body: Vec::new(),
            code_buf: Vec::new(),
            prn_enable,
            prn_pending: None,
            prn_lines: Vec::new(),
            current_pos: crate::error::SourcePos::new(source_file, 0),
        }
    }

    pub(super) fn error_code(&mut self, code: crate::error::ErrorCode, sym: Option<&[u8]>) {
        let err_ctx = match sym {
            Some(s) => crate::error::ErrorContext::with_symbol(self.current_pos.clone(), code, s),
            None => crate::error::ErrorContext::new(self.current_pos.clone(), code, None),
        };
        let mut stderr = std::io::stderr();
        crate::error::print_error_context(&mut stderr, &err_ctx);
        self.num_errors += 1;
    }

    /// code_buf を 10xx ブロックとして code_body にフラッシュする
    pub(super) fn flush_code_buf(&mut self) {
        if self.code_buf.is_empty() {
            return;
        }
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
                self.code_body
                    .push(if j + 1 < i + chunk { buf[j + 1] } else { 0 });
                j += 2;
            }
            i += chunk;
        }
    }

    /// dsb_pending を $3000 レコードとして code_body に書き出す（BSS/stack の .ds 用）
    pub(super) fn flush_dsb(&mut self) {
        if self.dsb_pending > 0 {
            self.code_body.push(0x30);
            self.code_body.push(0x00);
            let size = self.dsb_pending;
            self.code_body.extend_from_slice(&size.to_be_bytes());
            self.dsb_pending = 0;
        }
    }

    pub(super) fn sect_idx(&self) -> usize {
        (self.cur_sect as usize).saturating_sub(1).min(9)
    }

    pub(super) fn location(&self) -> u32 {
        self.loc_ctr[self.sect_idx()]
    }

    pub(super) fn advance(&mut self, n: u32) {
        let idx = self.sect_idx();
        self.loc_ctr[idx] = self.loc_ctr[idx].wrapping_add(n);
    }

    pub(super) fn emit(&mut self, bytes: &[u8]) {
        let idx = self.sect_idx();
        // BSS/Stack セクション（bss, stack, rbss, rstack, rlbss, rlstack）はサイズのみ記録
        let bss_like = matches!(self.cur_sect, 3 | 4 | 6 | 7 | 9 | 10);
        if bss_like {
            self.dsb_pending += bytes.len() as u32;
        } else {
            // pending の $3000 があればフラッシュしてから code_buf に追記
            if self.dsb_pending > 0 {
                self.flush_dsb();
            }
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

    pub(super) fn emit_zeros(&mut self, count: u32) {
        let idx = self.sect_idx();
        let bss_like = matches!(self.cur_sect, 3 | 4 | 6 | 7 | 9 | 10);
        if bss_like {
            self.dsb_pending += count;
        } else {
            // pending の $3000 があればフラッシュしてから code_buf に追記
            if self.dsb_pending > 0 {
                self.flush_dsb();
            }
            let zeros: Vec<u8> = vec![0u8; count as usize];
            self.sect_bytes[idx].extend_from_slice(&zeros);
            self.code_buf.extend_from_slice(&zeros);
        }
        self.advance(count);
    }

    /// .ds.X ディレクティブ用の予約スペース（全セクション対応）
    /// code_buf をフラッシュしてから dsb_pending に加算する。
    /// 次の emit/emit_zeros 呼び出し時に $3000 レコードとして出力される。
    pub(super) fn emit_reserve(&mut self, count: u32) {
        self.flush_code_buf();
        self.dsb_pending += count;
        self.advance(count);
    }

    /// 現在のPRNペンディング行をprn_linesにフラッシュする
    pub(super) fn prn_flush(&mut self) {
        if let Some((line_num, loc, sect, text, is_macro, bytes)) = self.prn_pending.take() {
            self.prn_lines.push(PrnLine {
                line_num,
                location: loc,
                section: sect,
                bytes,
                text,
                is_macro,
            });
        }
    }

    /// PRNペンディングを開始する
    pub(super) fn prn_start(&mut self, line_num: u32, text: Vec<u8>, is_macro: bool) {
        if self.prn_enable {
            self.prn_flush();
            let loc = self.location();
            let sect = self.cur_sect;
            self.prn_pending = Some((line_num, loc, sect, text, is_macro, Vec::new()));
        }
    }

    /// XDEF/Globl シンボルを最初の参照時点で ext_syms に登録する（B2xx順序をHASと一致させる）
    /// 既に登録済みの場合は何もしない。
    pub(super) fn try_register_xdef(&mut self, name: &Vec<u8>) {
        // 既に ext_syms に登録済みならスキップ
        if self.ext_syms.iter().any(|s| &s.name == name) {
            return;
        }
        if let Some(Symbol::Value {
            value,
            section,
            attrib,
            ext_attrib,
            ..
        }) = self.sym.lookup_sym(name)
        {
            let kind = match ext_attrib {
                ExtAttrib::XDef => {
                    if *attrib >= DefAttrib::Define {
                        *section
                    } else {
                        sym_kind::XDEF
                    }
                }
                ExtAttrib::Globl => {
                    if *attrib >= DefAttrib::Define {
                        *section
                    } else {
                        sym_kind::GLOBL
                    }
                }
                _ => return,
            };
            let val = if *attrib >= DefAttrib::Define {
                *value as u32
            } else {
                0
            };
            self.ext_syms.push(ExternalSymbol {
                kind,
                value: val,
                name: name.clone(),
            });
        }
    }

    /// 外部参照シンボルを検索し、なければ新規追加する。XREF通し番号（1ベース）を返す。
    /// RegSym エイリアスを自動的に解決する（例: abswarn → abswarn2）
    pub(super) fn get_or_add_xref(&mut self, name: Vec<u8>) -> u16 {
        // RegSym エイリアスチェーンを解決してから登録
        let resolved = resolve_regsym_chain(self.sym, &name);
        for sym in &self.ext_syms {
            if sym.kind == sym_kind::XREF && sym.name == resolved {
                return sym.value as u16;
            }
        }
        let num = self
            .ext_syms
            .iter()
            .filter(|s| s.kind == sym_kind::XREF)
            .count() as u32
            + 1;
        self.ext_syms.push(ExternalSymbol {
            kind: sym_kind::XREF,
            value: num,
            name: resolved,
        });
        num as u16
    }

    /// RPN 式を評価する
    pub(super) fn eval(&self, rpn: &Rpn) -> Result<EvalValue, Vec<u8>> {
        let loc = self.loc_top;
        let cur = self.location();
        let sec = self.cur_sect;
        let sym = self.sym;
        eval_rpn(rpn, loc, cur, sec, &|name| {
            sym.lookup_sym(name).and_then(sym_to_eval)
        })
        .map_err(|e| match e {
            crate::expr::eval::EvalError::UndefinedSymbol(n) => n,
            _ => b"<eval error>".to_vec(),
        })
    }
}

/// シンプルな絶対 XREF レコードを code_body に出力 (flush 済み前提)
pub(super) fn emit_abs_xref(code_body: &mut Vec<u8>, size: u8, xref_num: u16) {
    let tag = if size <= 2 { 0x41u8 } else { 0x42u8 };
    code_body.push(tag);
    code_body.push(0xFF);
    code_body.push((xref_num >> 8) as u8);
    code_body.push(xref_num as u8);
}

/// XREF + 定数オフセット ROFST レコードを code_body に出力 (flush 済み前提)
/// $50FF (byte) / $51FF (word) / $52FF (long) + xref_num + offset
pub(super) fn emit_rofst(code_body: &mut Vec<u8>, size: u8, xref_num: u16, offset: i32) {
    let tag = match size {
        1 => 0x50u8,
        2 => 0x51u8,
        _ => 0x52u8,
    };
    code_body.push(tag);
    code_body.push(0xFF);
    code_body.push((xref_num >> 8) as u8);
    code_body.push(xref_num as u8);
    code_body.extend_from_slice(&(offset as u32).to_be_bytes());
}

/// PC 相対 RPN リロケーションを code_body に出力 (flush 済み前提)
///
/// 原典 outpcdisp7 相当: XREF シンボルを RPN で参照し、ベースアドレスを
/// 減算してディスプレースメントを求める。リンカが最終解決を行う。
///   $80FF xref_num          — XREF シンボル値をスタックに積む
///   $80xx base_addr(4B)     — ベースアドレス（section 付き）をスタックに積む
///   $A00F                   — 減算（target - base = displacement）
///   $92/$99/$93 $00         — サイズ終端（long/word/short）
pub(super) fn emit_pc_rel_rpn(ctx: &mut P3Ctx<'_>, xref_num: u16, base_addr: u32, size_code: u8) {
    // XREF シンボル
    ctx.code_body.push(0x80);
    ctx.code_body.push(0xFF);
    ctx.code_body.push((xref_num >> 8) as u8);
    ctx.code_body.push(xref_num as u8);
    // ベースアドレス (section + 4バイトアドレス)
    ctx.code_body.push(0x80);
    ctx.code_body.push(ctx.cur_sect);
    ctx.code_body.extend_from_slice(&base_addr.to_be_bytes());
    // 減算演算子 (Sub = 0x0F)
    ctx.code_body.push(0xA0);
    ctx.code_body.push(0x0F);
    // サイズ終端
    ctx.code_body.push(size_code);
    ctx.code_body.push(0x00);
}

/// RPN 式を HLK RPN 式レコードとして code_body に出力
///
/// $80FF xref_num (外部), $80xx value (内部/定数), $A0xx (演算子), $9x00 (終端)
pub(super) fn emit_rpn_expression(ctx: &mut P3Ctx<'_>, rpn: &Rpn, size: u8) {
    for tok in rpn {
        match tok {
            RPNToken::SymbolRef(name) => match ctx.sym.lookup_sym(name).and_then(sym_to_eval) {
                Some(v) if v.is_constant() => {
                    ctx.code_body.push(0x80);
                    ctx.code_body.push(0x00);
                    ctx.code_body
                        .extend_from_slice(&(v.value as u32).to_be_bytes());
                }
                Some(v) => {
                    ctx.code_body.push(0x80);
                    ctx.code_body.push(v.section);
                    ctx.code_body
                        .extend_from_slice(&(v.value as u32).to_be_bytes());
                }
                None => {
                    let xref_num = ctx.get_or_add_xref(name.clone());
                    ctx.code_body.push(0x80);
                    ctx.code_body.push(0xFF);
                    ctx.code_body.push((xref_num >> 8) as u8);
                    ctx.code_body.push(xref_num as u8);
                }
            },
            RPNToken::Location => {
                ctx.code_body.push(0x80);
                ctx.code_body.push(ctx.cur_sect);
                ctx.code_body.extend_from_slice(&ctx.loc_top.to_be_bytes());
            }
            RPNToken::CurrentLoc => {
                ctx.code_body.push(0x80);
                ctx.code_body.push(ctx.cur_sect);
                ctx.code_body
                    .extend_from_slice(&ctx.location().to_be_bytes());
            }
            RPNToken::ValueByte(v) => {
                ctx.code_body.push(0x80);
                ctx.code_body.push(0x00);
                ctx.code_body.extend_from_slice(&(*v as u32).to_be_bytes());
            }
            RPNToken::ValueWord(v) => {
                ctx.code_body.push(0x80);
                ctx.code_body.push(0x00);
                ctx.code_body.extend_from_slice(&(*v as u32).to_be_bytes());
            }
            RPNToken::Value(v) => {
                ctx.code_body.push(0x80);
                ctx.code_body.push(0x00);
                ctx.code_body.extend_from_slice(&v.to_be_bytes());
            }
            RPNToken::Op(op) => {
                ctx.code_body.push(0xA0);
                ctx.code_body.push(*op as u8);
            }
            RPNToken::End => {
                let sz_code: u8 = match size {
                    1 => 0,
                    2 => 1,
                    _ => 2,
                };
                ctx.code_body.push(0x90 | sz_code);
                ctx.code_body.push(0x00);
                break;
            }
        }
    }
    ctx.advance(size as u32);
}

/// Pass3: TempRecord → ObjectCode + PRN行リスト
pub fn pass3(
    records: &[TempRecord],
    sym: &SymbolTable,
    source_name: Vec<u8>,
    source_file: Vec<u8>,
    prn_enable: bool,
    max_align: u8,
) -> (ObjectCode, Vec<PrnLine>, u32, u32) {
    let mut ctx = P3Ctx::new(sym, prn_enable, source_file.clone());
    let mut obj = ObjectCode::new(source_name);
    obj.source_file = source_file;
    if max_align > 0 {
        obj.has_align = true;
        obj.max_align = max_align;
    }

    for rec in records {
        ctx.loc_top = ctx.location();

        match rec {
            TempRecord::PositionMarker(pos) => {
                ctx.current_pos = pos.clone();
            }

            TempRecord::Const(bytes) => {
                ctx.emit(bytes);
            }

            TempRecord::DeferredInsn {
                base,
                handler,
                size,
                ops,
                ..
            } => {
                // B2xx 順序: XDEF シンボルを参照時点で先行登録
                for ea in ops.iter() {
                    register_xdefs_in_ea(&mut ctx, ea);
                }
                process_deferred(&mut ctx, *base, *handler, *size, ops);
            }

            TempRecord::Branch {
                opcode,
                target,
                cur_size,
                suppressed,
                ..
            } => {
                register_xdefs_in_rpn(&mut ctx, target);
                process_branch(&mut ctx, *opcode, target, *cur_size, *suppressed);
            }

            TempRecord::Data { size, rpn } => {
                register_xdefs_in_rpn(&mut ctx, rpn);
                match ctx.eval(rpn) {
                    Ok(v) => {
                        let bytes = val_to_bytes(v.value, *size);
                        ctx.emit(&bytes);
                    }
                    Err(_) => {
                        if let Some(name) = is_simple_external(rpn) {
                            // シンプルな外部参照 → ABS リロケーションレコード
                            let xref_num = ctx.get_or_add_xref(name.clone());
                            ctx.flush_code_buf();
                            ctx.flush_dsb();
                            emit_abs_xref(&mut ctx.code_body, *size, xref_num);
                            ctx.advance(*size as u32);
                        } else if let Some((name, offset)) = is_external_with_offset(rpn, ctx.sym) {
                            // XREF + 定数オフセット → ROFST レコード
                            let xref_num = ctx.get_or_add_xref(name.clone());
                            ctx.flush_code_buf();
                            ctx.flush_dsb();
                            emit_rofst(&mut ctx.code_body, *size, xref_num, offset);
                            ctx.advance(*size as u32);
                        } else {
                            // 複合外部式 → RPN 式レコード
                            ctx.flush_code_buf();
                            ctx.flush_dsb();
                            emit_rpn_expression(&mut ctx, rpn, *size);
                        }
                    }
                }
            }

            TempRecord::Ds { byte_count } => {
                ctx.emit_reserve(*byte_count);
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
                    if remaining == 1 {
                        fill.push(0x00);
                    } // 1バイト端数は常に0x00（NOP半分は無効）
                    ctx.emit(&fill);
                }
            }

            TempRecord::LabelDef { .. } => {
                // シンボル値は Pass2 で確定済み → 何もしない
            }
            TempRecord::EquDef { .. } => {
                // .equ/.set はシンボルテーブル更新のみ
            }

            TempRecord::SectChange { id } => {
                // code_buf をフラッシュし、BSS系の DSB ペンディングを出力してからセクション切り替え
                ctx.flush_code_buf();
                ctx.flush_dsb();
                ctx.cur_sect = *id;
                // 全セクションに $20xx レコードを出力（BSS/stack も含む）
                ctx.code_body.push(0x20);
                ctx.code_body.push(*id);
                ctx.code_body.extend_from_slice(&[0, 0, 0, 0]);
            }

            TempRecord::Org { value } => {
                let idx = ctx.sect_idx();
                ctx.loc_ctr[idx] = *value;
            }

            TempRecord::XDef { name } => {
                // 既にコード中の参照で先行登録済みならスキップ（B2xx順序をHASと一致させる）
                if ctx.ext_syms.iter().any(|s| &s.name == name) {
                    // already registered via try_register_xdef
                } else {
                    // コード中で参照されなかった XDEF: ここで初めて登録
                    let (val, kind) = if let Some(Symbol::Value {
                        value,
                        section,
                        attrib,
                        ..
                    }) = sym.lookup_sym(name)
                    {
                        if *attrib >= DefAttrib::Define {
                            (*value as u32, *section)
                        } else {
                            (0, sym_kind::XDEF)
                        }
                    } else {
                        (0, sym_kind::XDEF)
                    };
                    ctx.ext_syms.push(ExternalSymbol {
                        kind,
                        value: val,
                        name: name.clone(),
                    });
                }
            }

            TempRecord::XRef { name } => {
                // 外部参照シンボル番号を割り当て（XREF のみカウント、1から連番）
                // 既に同名の XREF が登録済みならスキップ（.reg 経由の先行登録との重複防止）
                if ctx
                    .ext_syms
                    .iter()
                    .any(|s| s.kind == sym_kind::XREF && &s.name == name)
                {
                    // already registered
                } else {
                    let num = ctx
                        .ext_syms
                        .iter()
                        .filter(|s| s.kind == sym_kind::XREF)
                        .count() as u32
                        + 1;
                    ctx.ext_syms.push(ExternalSymbol {
                        kind: sym_kind::XREF,
                        value: num,
                        name: name.clone(),
                    });
                }
            }

            TempRecord::Globl { name } => {
                // 既にコード中の参照で先行登録済みならスキップ
                if ctx.ext_syms.iter().any(|s| &s.name == name) {
                    // already registered via try_register_xdef
                } else {
                    // .globl / `::` ラベル — 定義済みなら実セクション番号、未定義なら XRef
                    let (val, kind) = if let Some(s) = sym.lookup_sym(name) {
                        if let Symbol::Value {
                            value,
                            section,
                            attrib,
                            ..
                        } = s
                        {
                            if *attrib >= DefAttrib::Define {
                                (*value as u32, *section) // 実セクション番号で出力
                            } else {
                                // 未定義 → XRef
                                let num = ctx
                                    .ext_syms
                                    .iter()
                                    .filter(|s| s.kind == sym_kind::XREF)
                                    .count() as u32
                                    + 1;
                                (num, sym_kind::XREF)
                            }
                        } else {
                            (0, sym_kind::GLOBL)
                        }
                    } else {
                        let num = ctx
                            .ext_syms
                            .iter()
                            .filter(|s| s.kind == sym_kind::XREF)
                            .count() as u32
                            + 1;
                        (num, sym_kind::XREF)
                    };
                    ctx.ext_syms.push(ExternalSymbol {
                        kind,
                        value: val,
                        name: name.clone(),
                    });
                }
            }

            TempRecord::Comm { name, ext } => {
                if !ctx.ext_syms.iter().any(|s| &s.name == name) {
                    let value = match sym.lookup_sym(name) {
                        Some(Symbol::Value { value, .. }) => *value as u32,
                        _ => 0,
                    };
                    let kind = match ext {
                        ExtAttrib::Comm => sym_kind::COMM,
                        ExtAttrib::RComm => sym_kind::R_COMM,
                        ExtAttrib::RLComm => sym_kind::RL_COMM,
                        _ => sym_kind::COMM,
                    };
                    ctx.ext_syms.push(ExternalSymbol {
                        kind,
                        value,
                        name: name.clone(),
                    });
                }
            }

            TempRecord::End => {
                break;
            }

            TempRecord::Cpu { .. } => {
                // CPU 変更は Pass1/Pass2 で処理済み
            }

            TempRecord::LineInfo {
                line_num,
                text,
                is_macro,
            } => {
                ctx.prn_start(*line_num, text.clone(), *is_macro);
            }
            TempRecord::ScdLn { line, loc } => {
                let (location, section) = match ctx.eval(loc) {
                    Ok(v) => (
                        v.value as u32,
                        if v.section == 0 {
                            ctx.cur_sect
                        } else {
                            v.section
                        },
                    ),
                    Err(_) => (ctx.location(), ctx.cur_sect),
                };
                obj.scd_events.push(ScdEvent::Ln {
                    line: *line,
                    location,
                    section,
                });
            }
            TempRecord::ScdAutoLn { line, loc } => {
                let (location, section) = match ctx.eval(loc) {
                    Ok(v) => (
                        v.value as u32,
                        if v.section == 0 {
                            ctx.cur_sect
                        } else {
                            v.section
                        },
                    ),
                    Err(_) => (ctx.location(), ctx.cur_sect),
                };
                obj.scd_events.push(ScdEvent::Ln {
                    line: *line,
                    location,
                    section,
                });
            }
            TempRecord::ScdVal { rpn } => {
                // HAS互換: .val はオブジェクト生成段階で式評価した値を保持する。
                let (value, section) = match ctx.eval(rpn) {
                    Ok(v) => {
                        let sec = if v.section == 0 { -1 } else { v.section as i16 };
                        (v.value as u32, sec)
                    }
                    Err(_) => (0, -2),
                };
                obj.scd_events.push(ScdEvent::Val { value, section });
            }
            TempRecord::ScdTag { name } => {
                obj.scd_events.push(ScdEvent::Tag { name: name.clone() });
            }
            TempRecord::ScdEndef {
                name,
                attrib,
                value,
                section,
                scl,
                type_code,
                size,
                dim,
                is_long,
            } => {
                obj.scd_events.push(ScdEvent::Endef {
                    name: name.clone(),
                    attrib: *attrib,
                    value: *value,
                    section: *section,
                    scl: *scl,
                    type_code: *type_code,
                    size: *size,
                    dim: *dim,
                    is_long: *is_long,
                });
            }
            TempRecord::ScdFuncEnd { location, section } => {
                obj.scd_events.push(ScdEvent::FuncEnd {
                    location: *location,
                    section: *section,
                });
            }
        }
    }

    // 最後のPRN行をフラッシュ
    if ctx.prn_enable {
        ctx.prn_flush();
    }

    // 残りの code_buf と DSB ペンディングをフラッシュ
    ctx.flush_code_buf();
    ctx.flush_dsb();
    obj.code_body = std::mem::take(&mut ctx.code_body);

    let prn_lines = ctx.prn_lines;

    // セクション情報を構築（常に text/data/bss/stack の4セクションを出力）
    for sect_id in 1u8..=4 {
        let idx = (sect_id as usize) - 1;
        let bytes = &ctx.sect_bytes[idx];
        // 外部参照（XREFリロケーション）があるとsect_bytes.len() < loc_ctrになるため、
        // 全セクションでロケーションカウンタを正規サイズとして使う
        let size = ctx.loc_ctr[idx];
        obj.sections.push(SectionInfo {
            id: sect_id,
            bytes: if sect_id >= 3 {
                Vec::new()
            } else {
                bytes.clone()
            },
            size,
        });
    }
    // 相対セクション（5〜7: rdata/rbss/rstack）は使用時のみ追加
    // 対応する rl* セクション（8〜10: rldata/rlbss/rlstack）も同時に追加
    let mut rsect_used = [false; 3]; // rdata, rbss, rstack
    for (i, sect_id) in [5u8, 6, 7].iter().enumerate() {
        let idx = (*sect_id as usize) - 1;
        let bytes = &ctx.sect_bytes[idx];
        let size = ctx.loc_ctr[idx];
        if size > 0 || !bytes.is_empty() {
            rsect_used[i] = true;
            let is_bss = matches!(sect_id, 6 | 7);
            obj.sections.push(SectionInfo {
                id: *sect_id,
                bytes: if is_bss { Vec::new() } else { bytes.clone() },
                size,
            });
        }
    }
    // rl* セクション: 対応する r* セクションが使われていれば常に出力
    for (i, sect_id) in [8u8, 9, 10].iter().enumerate() {
        let idx = (*sect_id as usize) - 1;
        let bytes = &ctx.sect_bytes[idx];
        let size = ctx.loc_ctr[idx];
        if rsect_used[i] || size > 0 || !bytes.is_empty() {
            let is_bss = matches!(sect_id, 9 | 10);
            obj.sections.push(SectionInfo {
                id: *sect_id,
                bytes: if is_bss { Vec::new() } else { bytes.clone() },
                size,
            });
        }
    }

    obj.ext_syms = ctx.ext_syms;
    (obj, prn_lines, ctx.num_errors, 0)
}
