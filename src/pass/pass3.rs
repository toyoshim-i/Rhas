/// Pass 3: オブジェクトコード生成
///
/// TempRecord 列とシンボルテーブルから最終的なバイト列と
/// 外部シンボル情報を生成し、ObjectCode を返す。

use crate::addressing::{Displacement, EffectiveAddress};
use crate::expr::{eval_rpn, Rpn};
use crate::expr::eval::EvalValue;
use crate::expr::rpn::{Operator, RPNToken};
use crate::instructions::encode_insn;
use crate::object::{ExternalSymbol, ObjectCode, ScdEvent, SectionInfo, sym_kind};
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
// EA 外部参照種別
// ----------------------------------------------------------------

/// EA に含まれる外部参照の種別
enum EaExtKind {
    /// シンプルな絶対外部参照: $41/$42 FF xref_num
    SimpleAbs(Vec<u8>),
    /// 外部参照 + 定数オフセット: $50/$51/$52 FF xref_num offset4 (ROFST形式)
    ExtWithOffset(Vec<u8>, i32),
    /// PC相対外部参照: $65 sect loc4 xref_num
    PcRel(Vec<u8>),
    /// 複合外部式: RPN フォーマット
    Complex(Rpn),
}

#[derive(Clone)]
struct FoldExpr {
    name: Option<Vec<u8>>,
    offset: i32,
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
    /// 現在のセクションでフラッシュ待ちの DSB サイズ（.ds in BSS/stack → $3000 レコードへ）
    dsb_pending: u32,
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

    /// dsb_pending を $3000 レコードとして code_body に書き出す（BSS/stack の .ds 用）
    fn flush_dsb(&mut self) {
        if self.dsb_pending > 0 {
            self.code_body.push(0x30);
            self.code_body.push(0x00);
            let size = self.dsb_pending;
            self.code_body.extend_from_slice(&size.to_be_bytes());
            self.dsb_pending = 0;
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
            self.dsb_pending += bytes.len() as u32;
        } else {
            // pending の $3000 があればフラッシュしてから code_buf に追記
            if self.dsb_pending > 0 { self.flush_dsb(); }
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
            self.dsb_pending += count;
        } else {
            // pending の $3000 があればフラッシュしてから code_buf に追記
            if self.dsb_pending > 0 { self.flush_dsb(); }
            let zeros: Vec<u8> = vec![0u8; count as usize];
            self.sect_bytes[idx].extend_from_slice(&zeros);
            self.code_buf.extend_from_slice(&zeros);
        }
        self.advance(count);
    }

    /// .ds.X ディレクティブ用の予約スペース（全セクション対応）
    /// code_buf をフラッシュしてから dsb_pending に加算する。
    /// 次の emit/emit_zeros 呼び出し時に $3000 レコードとして出力される。
    fn emit_reserve(&mut self, count: u32) {
        self.flush_code_buf();
        self.dsb_pending += count;
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

    /// XDEF/Globl シンボルを最初の参照時点で ext_syms に登録する（B2xx順序をHASと一致させる）
    /// 既に登録済みの場合は何もしない。
    fn try_register_xdef(&mut self, name: &Vec<u8>) {
        // 既に ext_syms に登録済みならスキップ
        if self.ext_syms.iter().any(|s| &s.name == name) {
            return;
        }
        if let Some(Symbol::Value { value, section, attrib, ext_attrib, .. }) = self.sym.lookup_sym(name) {
            let kind = match ext_attrib {
                ExtAttrib::XDef => {
                    if *attrib >= DefAttrib::Define { *section } else { sym_kind::XDEF }
                }
                ExtAttrib::Globl => {
                    if *attrib >= DefAttrib::Define { *section } else { sym_kind::GLOBL }
                }
                _ => return,
            };
            let val = if *attrib >= DefAttrib::Define { *value as u32 } else { 0 };
            self.ext_syms.push(ExternalSymbol { kind, value: val, name: name.clone() });
        }
    }

    /// 外部参照シンボルを検索し、なければ新規追加する。XREF通し番号（1ベース）を返す。
    /// RegSym エイリアスを自動的に解決する（例: abswarn → abswarn2）
    fn get_or_add_xref(&mut self, name: Vec<u8>) -> u16 {
        // RegSym エイリアスチェーンを解決してから登録
        let resolved = resolve_regsym_chain(self.sym, &name);
        for sym in &self.ext_syms {
            if sym.kind == sym_kind::XREF && sym.name == resolved {
                return sym.value as u16;
            }
        }
        let num = self.ext_syms.iter()
            .filter(|s| s.kind == sym_kind::XREF)
            .count() as u32 + 1;
        self.ext_syms.push(ExternalSymbol { kind: sym_kind::XREF, value: num, name: resolved });
        num as u16
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

/// RegSym エイリアスチェーンをたどって最終的なシンボル名を返す（所有値）
///
/// `abswarn reg abswarn2` の場合: "abswarn" → "abswarn2"
/// チェーンが RegSym でなくなったとき、またはループ上限に達したとき終了。
fn resolve_regsym_chain(sym: &SymbolTable, name: &[u8]) -> Vec<u8> {
    let mut current: Vec<u8> = name.to_vec();
    let mut depth = 0u8;
    loop {
        if depth >= 16 { return current; }
        match sym.lookup_sym(&current) {
            Some(Symbol::RegSym { define }) if define.len() == 1 && define[0].len() >= 2 => {
                if let (RPNToken::SymbolRef(target), RPNToken::End) =
                    (&define[0][define[0].len() - 2], &define[0][define[0].len() - 1])
                {
                    if define[0].len() == 2 {
                        // シンプルな 1シンボル参照: [SymbolRef(target), End]
                        current = target.clone();
                        depth += 1;
                        continue;
                    }
                }
            }
            _ => {}
        }
        return current;
    }
}

/// RPN がシンプルな外部参照 [SymbolRef(name), End] かチェック
fn is_simple_external(rpn: &Rpn) -> Option<&Vec<u8>> {
    if rpn.len() == 2 {
        if let (RPNToken::SymbolRef(name), RPNToken::End) = (&rpn[0], &rpn[1]) {
            return Some(name);
        }
    }
    None
}

/// RPN トークンを定数として評価する（数値リテラルまたは定数 .equ シンボル）
fn token_as_const(tok: &RPNToken, sym: &SymbolTable) -> Option<i32> {
    match tok {
        RPNToken::Value(v)     => Some(*v as i32),
        RPNToken::ValueWord(v) => Some(*v as i32),
        RPNToken::ValueByte(v) => Some(*v as i32),
        RPNToken::SymbolRef(name) => {
            // .equ 定数シンボルの場合は値を取得
            sym.lookup_sym(name).and_then(sym_to_eval)
                .filter(|v| v.is_constant())
                .map(|v| v.value as i32)
        }
        _ => None,
    }
}

/// RPN が「単一 XREF + 定数オフセット」に簡約できるかチェック。
///
/// 例:
/// - `sym + 4`
/// - `4 + sym`
/// - `sym + (16*4)`  // 定数部分は先に畳み込む
fn is_external_with_offset(rpn: &Rpn, sym: &SymbolTable) -> Option<(Vec<u8>, i32)> {
    let mut stack: Vec<FoldExpr> = Vec::new();

    for tok in rpn {
        match tok {
            RPNToken::End => break,
            RPNToken::Value(v) => stack.push(FoldExpr { name: None, offset: *v as i32 }),
            RPNToken::ValueWord(v) => stack.push(FoldExpr { name: None, offset: *v as i32 }),
            RPNToken::ValueByte(v) => stack.push(FoldExpr { name: None, offset: *v as i32 }),
            RPNToken::SymbolRef(name) => {
                if let Some(v) = sym.lookup_sym(name).and_then(sym_to_eval).filter(|v| v.is_constant()) {
                    stack.push(FoldExpr { name: None, offset: v.value });
                } else {
                    stack.push(FoldExpr { name: Some(name.clone()), offset: 0 });
                }
            }
            RPNToken::Op(op) => {
                let rhs = stack.pop()?;
                let lhs = stack.pop()?;
                let merged = match op {
                    Operator::Add => fold_add(lhs, rhs)?,
                    Operator::Sub => fold_sub(lhs, rhs)?,
                    Operator::Mul => fold_mul(lhs, rhs)?,
                    _ => return None,
                };
                stack.push(merged);
            }
            _ => return None,
        }
    }
    let out = stack.pop()?;
    if !stack.is_empty() {
        return None;
    }
    out.name.map(|n| (n, out.offset))
}

fn fold_add(lhs: FoldExpr, rhs: FoldExpr) -> Option<FoldExpr> {
    match (lhs.name, rhs.name) {
        (None, None) => Some(FoldExpr { name: None, offset: lhs.offset + rhs.offset }),
        (Some(n), None) => Some(FoldExpr { name: Some(n), offset: lhs.offset + rhs.offset }),
        (None, Some(n)) => Some(FoldExpr { name: Some(n), offset: lhs.offset + rhs.offset }),
        (Some(_), Some(_)) => None,
    }
}

fn fold_sub(lhs: FoldExpr, rhs: FoldExpr) -> Option<FoldExpr> {
    match (lhs.name, rhs.name) {
        (None, None) => Some(FoldExpr { name: None, offset: lhs.offset - rhs.offset }),
        (Some(n), None) => Some(FoldExpr { name: Some(n), offset: lhs.offset - rhs.offset }),
        _ => None,
    }
}

fn fold_mul(lhs: FoldExpr, rhs: FoldExpr) -> Option<FoldExpr> {
    match (lhs.name, rhs.name) {
        (None, None) => Some(FoldExpr { name: None, offset: lhs.offset * rhs.offset }),
        // (sym + k) * 1 は恒等
        (Some(n), None) if rhs.offset == 1 => Some(FoldExpr { name: Some(n), offset: lhs.offset }),
        (None, Some(n)) if lhs.offset == 1 => Some(FoldExpr { name: Some(n), offset: rhs.offset }),
        _ => None,
    }
}

/// RPN 内の全 SymbolRef に対して try_register_xdef を呼ぶ（B2xx 順序の先行登録）
fn register_xdefs_in_rpn(ctx: &mut P3Ctx<'_>, rpn: &Rpn) {
    for tok in rpn {
        if let RPNToken::SymbolRef(name) = tok {
            ctx.try_register_xdef(name);
        }
    }
}

/// EA 内の全 RPN に対して register_xdefs_in_rpn を呼ぶ
fn register_xdefs_in_ea(ctx: &mut P3Ctx<'_>, ea: &EffectiveAddress) {
    match ea {
        EffectiveAddress::Immediate(rpn) |
        EffectiveAddress::AbsShort(rpn) |
        EffectiveAddress::AbsLong(rpn) => register_xdefs_in_rpn(ctx, rpn),
        EffectiveAddress::AddrRegDisp { disp, .. } => register_xdefs_in_rpn(ctx, &disp.rpn),
        EffectiveAddress::AddrRegIdx { disp, .. } => register_xdefs_in_rpn(ctx, &disp.rpn),
        EffectiveAddress::PcDisp(disp) => register_xdefs_in_rpn(ctx, &disp.rpn),
        EffectiveAddress::PcIdx { disp, .. } => register_xdefs_in_rpn(ctx, &disp.rpn),
        _ => {}
    }
}

/// シンプルな絶対 XREF レコードを code_body に出力 (flush 済み前提)
fn emit_abs_xref(code_body: &mut Vec<u8>, size: u8, xref_num: u16) {
    let tag = if size <= 2 { 0x41u8 } else { 0x42u8 };
    code_body.push(tag);
    code_body.push(0xFF);
    code_body.push((xref_num >> 8) as u8);
    code_body.push(xref_num as u8);
}

/// XREF + 定数オフセット ROFST レコードを code_body に出力 (flush 済み前提)
/// $50FF (byte) / $51FF (word) / $52FF (long) + xref_num + offset
fn emit_rofst(code_body: &mut Vec<u8>, size: u8, xref_num: u16, offset: i32) {
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

/// RPN 式を HLK RPN 式レコードとして code_body に出力
///
/// $80FF xref_num (外部), $80xx value (内部/定数), $A0xx (演算子), $9x00 (終端)
fn emit_rpn_expression(ctx: &mut P3Ctx<'_>, rpn: &Rpn, size: u8) {
    for tok in rpn {
        match tok {
            RPNToken::SymbolRef(name) => {
                match ctx.sym.lookup_sym(name).and_then(sym_to_eval) {
                    Some(v) if v.is_constant() => {
                        ctx.code_body.push(0x80);
                        ctx.code_body.push(0x00);
                        ctx.code_body.extend_from_slice(&(v.value as u32).to_be_bytes());
                    }
                    Some(v) => {
                        ctx.code_body.push(0x80);
                        ctx.code_body.push(v.section);
                        ctx.code_body.extend_from_slice(&(v.value as u32).to_be_bytes());
                    }
                    None => {
                        let xref_num = ctx.get_or_add_xref(name.clone());
                        ctx.code_body.push(0x80);
                        ctx.code_body.push(0xFF);
                        ctx.code_body.push((xref_num >> 8) as u8);
                        ctx.code_body.push(xref_num as u8);
                    }
                }
            }
            RPNToken::Location => {
                ctx.code_body.push(0x80);
                ctx.code_body.push(ctx.cur_sect);
                ctx.code_body.extend_from_slice(&ctx.loc_top.to_be_bytes());
            }
            RPNToken::CurrentLoc => {
                ctx.code_body.push(0x80);
                ctx.code_body.push(ctx.cur_sect);
                ctx.code_body.extend_from_slice(&ctx.location().to_be_bytes());
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

// ----------------------------------------------------------------
// Pass3 メイン
// ----------------------------------------------------------------

/// Pass3: TempRecord → ObjectCode + PRN行リスト
pub fn pass3(
    records:     &[TempRecord],
    sym:         &SymbolTable,
    source_name: Vec<u8>,
    source_file: Vec<u8>,
    prn_enable:  bool,
    max_align:   u8,
) -> (ObjectCode, Vec<PrnLine>) {
    let mut ctx = P3Ctx::new(sym, prn_enable);
    let mut obj = ObjectCode::new(source_name);
    obj.source_file = source_file;
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
                // B2xx 順序: XDEF シンボルを参照時点で先行登録
                for ea in ops.iter() { register_xdefs_in_ea(&mut ctx, ea); }
                process_deferred(&mut ctx, *base, *handler, *size, ops);
            }

            TempRecord::Branch { opcode, target, cur_size, suppressed, .. } => {
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
                    if remaining == 1 { fill.push(0x00); } // 1バイト端数は常に0x00（NOP半分は無効）
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
                    let (val, kind) = if let Some(s) = sym.lookup_sym(name) {
                        if let Symbol::Value { value, section, attrib, .. } = s {
                            if *attrib >= DefAttrib::Define {
                                (*value as u32, *section)
                            } else { (0, sym_kind::XDEF) }
                        } else { (0, sym_kind::XDEF) }
                    } else { (0, sym_kind::XDEF) };
                    ctx.ext_syms.push(ExternalSymbol {
                        kind, value: val, name: name.clone()
                    });
                }
            }

            TempRecord::XRef { name } => {
                // 外部参照シンボル番号を割り当て（XREF のみカウント、1から連番）
                // 既に同名の XREF が登録済みならスキップ（.reg 経由の先行登録との重複防止）
                if ctx.ext_syms.iter().any(|s| s.kind == sym_kind::XREF && &s.name == name) {
                    // already registered
                } else {
                    let num = ctx.ext_syms.iter()
                        .filter(|s| s.kind == sym_kind::XREF)
                        .count() as u32 + 1;
                    ctx.ext_syms.push(ExternalSymbol {
                        kind: sym_kind::XREF, value: num, name: name.clone()
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
                        if let Symbol::Value { value, section, attrib, .. } = s {
                            if *attrib >= DefAttrib::Define {
                                (*value as u32, *section)  // 実セクション番号で出力
                            } else {
                                // 未定義 → XRef
                                let num = ctx.ext_syms.iter()
                                    .filter(|s| s.kind == sym_kind::XREF)
                                    .count() as u32 + 1;
                                (num, sym_kind::XREF)
                            }
                        } else { (0, sym_kind::GLOBL) }
                    } else {
                        let num = ctx.ext_syms.iter()
                            .filter(|s| s.kind == sym_kind::XREF)
                            .count() as u32 + 1;
                        (num, sym_kind::XREF)
                    };
                    ctx.ext_syms.push(ExternalSymbol {
                        kind, value: val, name: name.clone()
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
                        kind, value, name: name.clone()
                    });
                }
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
            TempRecord::ScdLn { line, loc } => {
                let (location, section) = match ctx.eval(loc) {
                    Ok(v) => (v.value as u32, if v.section == 0 { ctx.cur_sect } else { v.section }),
                    Err(_) => (ctx.location(), ctx.cur_sect),
                };
                obj.scd_events.push(ScdEvent::Ln { line: *line, location, section });
            }
            TempRecord::ScdAutoLn { line, loc } => {
                let (location, section) = match ctx.eval(loc) {
                    Ok(v) => (v.value as u32, if v.section == 0 { ctx.cur_sect } else { v.section }),
                    Err(_) => (ctx.location(), ctx.cur_sect),
                };
                obj.scd_events.push(ScdEvent::Ln { line: *line, location, section });
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
            TempRecord::ScdEndef { name, attrib, value, section, scl, type_code, size, dim, is_long } => {
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
            id:    sect_id,
            bytes: if sect_id >= 3 { Vec::new() } else { bytes.clone() },
            size,
        });
    }
    // 相対セクション（5〜10）は使用時のみ追加
    for sect_id in 5u8..=10 {
        let idx = (sect_id as usize) - 1;
        let bytes = &ctx.sect_bytes[idx];
        let size = ctx.loc_ctr[idx];
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
    // EA 内の RPN を評価し、外部参照は 0 に置き換えて種別を記録
    let mut resolved_ops: Vec<EffectiveAddress> = Vec::with_capacity(ops.len());
    let mut ext_info: Vec<Option<EaExtKind>> = Vec::with_capacity(ops.len());
    for ea in ops {
        let (resolved, ext_kind) = resolve_ea_with_ext(ctx, ea);
        ext_info.push(ext_kind);
        resolved_ops.push(resolved);
    }

    let has_ext = ext_info.iter().any(|e| e.is_some());

    // DBcc は encode_insn では常に DeferToLinker を返すため、ここで特別処理する
    if matches!(handler, InsnHandler::DBcc) {
        let pc = ctx.location();
        let dn = match resolved_ops.get(0) {
            Some(EffectiveAddress::DataReg(n)) => *n,
            _ => { ctx.emit_zeros(4); ctx.num_errors += 1; return; }
        };
        let opcode_word = base | (dn as u16);
        match ext_info.get(1) {
            Some(None) => {
                // 内部参照: resolve_ea_with_ext で解決済みの AbsLong からターゲットアドレスを取得
                let target_addr = match resolved_ops.get(1) {
                    Some(EffectiveAddress::AbsLong(rpn)) => {
                        match rpn.first() {
                            Some(RPNToken::Value(v)) => *v as i32,
                            _ => 0,
                        }
                    }
                    _ => 0,
                };
                let disp = target_addr - (pc as i32 + 2);
                if disp >= -32768 && disp <= 32767 {
                    let dw = disp as i16 as u16;
                    ctx.emit(&[(opcode_word >> 8) as u8, (opcode_word & 0xFF) as u8,
                               (dw >> 8) as u8, (dw & 0xFF) as u8]);
                } else {
                    ctx.emit_zeros(4);
                    ctx.num_errors += 1;
                }
            }
            Some(Some(EaExtKind::SimpleAbs(name))) => {
                // 外部参照: PC相対リロケーションレコードを生成
                let name = name.clone();
                let xref_num = ctx.get_or_add_xref(name);
                ctx.emit(&[(opcode_word >> 8) as u8, (opcode_word & 0xFF) as u8]);
                let loc = ctx.location(); // displacement word のアドレス
                ctx.flush_code_buf();
                ctx.flush_dsb();
                ctx.advance(2);
                let sect = ctx.cur_sect;
                ctx.code_body.extend_from_slice(&[0x65, sect]);
                ctx.code_body.extend_from_slice(&loc.to_be_bytes());
                ctx.code_body.push((xref_num >> 8) as u8);
                ctx.code_body.push(xref_num as u8);
            }
            _ => {
                ctx.emit_zeros(4);
                ctx.num_errors += 1;
            }
        }
        return;
    }

    // FBcc は PC 相対分岐をここで解決する
    if matches!(handler, InsnHandler::FBcc) {
        let pc = ctx.location();
        let target_addr = match ext_info.first() {
            Some(None) => match resolved_ops.first() {
                Some(EffectiveAddress::AbsLong(rpn)) | Some(EffectiveAddress::AbsShort(rpn)) => {
                    match rpn.first() {
                        Some(RPNToken::Value(v)) => *v as i32,
                        _ => { ctx.emit_zeros(4); ctx.num_errors += 1; return; }
                    }
                }
                _ => { ctx.emit_zeros(4); ctx.num_errors += 1; return; }
            },
            _ => { ctx.emit_zeros(4); ctx.num_errors += 1; return; } // 外部参照は未対応
        };
        let disp = target_addr - (pc as i32 + 2);
        let mut opcode_word = base;
        match size {
            SizeCode::Long => {
                opcode_word |= 0x0040;
                ctx.emit(&[
                    (opcode_word >> 8) as u8, (opcode_word & 0xFF) as u8,
                    ((disp >> 24) & 0xFF) as u8, ((disp >> 16) & 0xFF) as u8,
                    ((disp >> 8) & 0xFF) as u8, (disp & 0xFF) as u8,
                ]);
            }
            SizeCode::Word | SizeCode::None => {
                if !(-32768..=32767).contains(&disp) {
                    if matches!(size, SizeCode::None) {
                        opcode_word |= 0x0040;
                        ctx.emit(&[
                            (opcode_word >> 8) as u8, (opcode_word & 0xFF) as u8,
                            ((disp >> 24) & 0xFF) as u8, ((disp >> 16) & 0xFF) as u8,
                            ((disp >> 8) & 0xFF) as u8, (disp & 0xFF) as u8,
                        ]);
                    } else {
                        ctx.emit_zeros(4);
                        ctx.num_errors += 1;
                    }
                } else {
                    let dw = disp as i16 as u16;
                    ctx.emit(&[
                        (opcode_word >> 8) as u8, (opcode_word & 0xFF) as u8,
                        (dw >> 8) as u8, (dw & 0xFF) as u8,
                    ]);
                }
            }
            _ => {
                ctx.emit_zeros(4);
                ctx.num_errors += 1;
            }
        }
        return;
    }

    // FDBcc は opcode + cond + disp16 をここで解決する
    if matches!(handler, InsnHandler::FDBcc) {
        let pc = ctx.location();
        let dn = match resolved_ops.first() {
            Some(EffectiveAddress::DataReg(n)) => *n,
            _ => { ctx.emit_zeros(6); ctx.num_errors += 1; return; }
        };
        let target_addr = match ext_info.get(1) {
            Some(None) => match resolved_ops.get(1) {
                Some(EffectiveAddress::AbsLong(rpn)) | Some(EffectiveAddress::AbsShort(rpn)) => {
                    match rpn.first() {
                        Some(RPNToken::Value(v)) => *v as i32,
                        _ => { ctx.emit_zeros(6); ctx.num_errors += 1; return; }
                    }
                }
                _ => { ctx.emit_zeros(6); ctx.num_errors += 1; return; }
            },
            _ => { ctx.emit_zeros(6); ctx.num_errors += 1; return; } // 外部参照は未対応
        };
        let opcode_word = 0xF048u16 | (base & 0x0E00) | (dn as u16);
        let cond_word = base & 0x001F;
        let disp = target_addr - (pc as i32 + 4);
        if !(-32768..=32767).contains(&disp) {
            ctx.emit_zeros(6);
            ctx.num_errors += 1;
            return;
        }
        let dw = disp as i16 as u16;
        ctx.emit(&[
            (opcode_word >> 8) as u8, (opcode_word & 0xFF) as u8,
            (cond_word >> 8) as u8, (cond_word & 0xFF) as u8,
            (dw >> 8) as u8, (dw & 0xFF) as u8,
        ]);
        return;
    }

    match encode_insn(base, handler, size, &resolved_ops) {
        Ok(bytes) => {
            if !has_ext {
                // 外部参照なし → そのまま出力
                ctx.emit(&bytes);
                return;
            }
            // 外部参照あり → バイト列を分割してリロケーションレコードを挿入
            // bytes = opcode(2) + op0_ext + op1_ext + ...
            // まずオペコードワード (2 bytes) を出力
            ctx.emit(&bytes[..2]);
            let mut pos = 2usize;
            for (i, ea) in resolved_ops.iter().enumerate() {
                // SubAddQ (ADDQ/SUBQ): immediate count is embedded in opcode bits, not as extension word
                let ext_sz = if matches!(handler, InsnHandler::SubAddQ) && i == 0 {
                    0
                } else {
                    ea_ext_size_for_insn(ea, size) as usize
                };
                if ext_sz == 0 { continue; }
                match &ext_info[i] {
                    Some(EaExtKind::SimpleAbs(name)) => {
                        // シンプルな絶対外部参照 → $41/$42 FF xref_num
                        let xref_num = ctx.get_or_add_xref(name.clone());
                        ctx.flush_code_buf();
                        ctx.flush_dsb();
                        emit_abs_xref(&mut ctx.code_body, ext_sz as u8, xref_num);
                        ctx.advance(ext_sz as u32);
                    }
                    Some(EaExtKind::ExtWithOffset(name, offset)) => {
                        // XREF + 定数オフセット → ROFST レコード
                        let xref_num = ctx.get_or_add_xref(name.clone());
                        let offset = *offset;
                        ctx.flush_code_buf();
                        ctx.flush_dsb();
                        emit_rofst(&mut ctx.code_body, ext_sz as u8, xref_num, offset);
                        ctx.advance(ext_sz as u32);
                    }
                    Some(EaExtKind::PcRel(name)) => {
                        // PC相対外部参照 → $65 sect loc4 xref_num
                        // オペコードバイトはすでに code_buf に入っている
                        let xref_num = ctx.get_or_add_xref(name.clone());
                        let loc = ctx.location();  // displacement スロットのアドレス
                        ctx.flush_code_buf();
                        ctx.flush_dsb();
                        ctx.advance(ext_sz as u32);  // displacement スロット分進める
                        let sect = ctx.cur_sect;
                        ctx.code_body.extend_from_slice(&[0x65, sect]);
                        ctx.code_body.extend_from_slice(&loc.to_be_bytes());
                        ctx.code_body.push((xref_num >> 8) as u8);
                        ctx.code_body.push(xref_num as u8);
                    }
                    Some(EaExtKind::Complex(rpn)) => {
                        // 複合外部式 → RPN 式レコード
                        let rpn = rpn.clone();
                        ctx.flush_code_buf();
                        ctx.flush_dsb();
                        emit_rpn_expression(ctx, &rpn, ext_sz as u8);
                    }
                    None => {
                        // 内部参照 → バイトをそのまま出力
                        if pos + ext_sz <= bytes.len() {
                            ctx.emit(&bytes[pos..pos + ext_sz]);
                        }
                    }
                }
                pos += ext_sz;
            }
        }
        Err(_) => {
            // 未解決のまま → ゼロバイトで埋める
            let est = 2 + resolved_ops.iter().map(|ea| ea_ext_size_for_insn(ea, size)).sum::<u32>();
            ctx.emit_zeros(est);
            ctx.num_errors += 1;
        }
    }
}

/// EA 内の RPN 式を評価して定数 EA を返す。外部参照の場合は (EA_with_zero, Some(EaExtKind)) を返す。
fn resolve_ea_with_ext(ctx: &P3Ctx<'_>, ea: &EffectiveAddress) -> (EffectiveAddress, Option<EaExtKind>) {
    let zero_rpn = || vec![RPNToken::Value(0u32), RPNToken::End];

    /// RPN 評価を試み、外部参照の場合に EaExtKind を決定する
    let classify_ext = |rpn: &Rpn| -> EaExtKind {
        if let Some(name) = is_simple_external(rpn) {
            EaExtKind::SimpleAbs(name.clone())
        } else if let Some((name, offset)) = is_external_with_offset(rpn, ctx.sym) {
            EaExtKind::ExtWithOffset(name.clone(), offset)
        } else {
            EaExtKind::Complex(rpn.clone())
        }
    };

    match ea {
        EffectiveAddress::Immediate(rpn) => {
            match ctx.eval(rpn) {
                Ok(v) => (EffectiveAddress::Immediate(vec![RPNToken::Value(v.value as u32), RPNToken::End]), None),
                Err(_) => (EffectiveAddress::Immediate(zero_rpn()), Some(classify_ext(rpn))),
            }
        }
        EffectiveAddress::AbsShort(rpn) => {
            match ctx.eval(rpn) {
                Ok(v) => (EffectiveAddress::AbsShort(vec![RPNToken::Value(v.value as u32), RPNToken::End]), None),
                Err(name) => (EffectiveAddress::AbsShort(zero_rpn()), Some(EaExtKind::SimpleAbs(name))),
            }
        }
        EffectiveAddress::AbsLong(rpn) => {
            match ctx.eval(rpn) {
                Ok(v) => (EffectiveAddress::AbsLong(vec![RPNToken::Value(v.value as u32), RPNToken::End]), None),
                Err(name) => (EffectiveAddress::AbsLong(zero_rpn()), Some(EaExtKind::SimpleAbs(name))),
            }
        }
        EffectiveAddress::AddrRegDisp { an, disp } => {
            if disp.const_val.is_some() || disp.rpn.is_empty() {
                (ea.clone(), None)
            } else {
                match ctx.eval(&disp.rpn) {
                    Ok(v) => {
                        let new_disp = Displacement {
                            rpn: vec![RPNToken::Value(v.value as u32), RPNToken::End],
                            size: disp.size,
                            const_val: Some(v.value),
                        };
                        (EffectiveAddress::AddrRegDisp { an: *an, disp: new_disp }, None)
                    }
                    Err(_) => {
                        let new_disp = Displacement {
                            rpn: zero_rpn(),
                            size: disp.size,
                            // Note: Use non-zero placeholder to prevent (0,An)→(An) optimization.
                            // The actual value doesn't matter since the relocation record overwrites it.
                            const_val: Some(1),
                        };
                        (EffectiveAddress::AddrRegDisp { an: *an, disp: new_disp }, Some(classify_ext(&disp.rpn)))
                    }
                }
            }
        }
        EffectiveAddress::PcDisp(disp) => {
            if disp.const_val.is_some() || disp.rpn.is_empty() {
                (ea.clone(), None)
            } else {
                match ctx.eval(&disp.rpn) {
                    Ok(v) => {
                        // displacement = target_addr - displacement_word_addr
                        // displacement_word_addr = 命令先頭 + 2 (オペコードワード分)
                        let target_addr = v.value;
                        let disp_word_addr = ctx.location() as i32 + 2;
                        let displacement = target_addr - disp_word_addr;
                        let new_disp = Displacement {
                            rpn: vec![RPNToken::Value(displacement as u32), RPNToken::End],
                            size: disp.size,
                            const_val: Some(displacement),
                        };
                        (EffectiveAddress::PcDisp(new_disp), None)
                    }
                    Err(name) => {
                        // PC相対外部参照: displacement=0 でエンコード、$65 リロケーションを生成
                        let new_disp = Displacement {
                            rpn: zero_rpn(),
                            size: disp.size,
                            const_val: Some(0),
                        };
                        (EffectiveAddress::PcDisp(new_disp), Some(EaExtKind::PcRel(name)))
                    }
                }
            }
        }
        EffectiveAddress::PcIdx { disp, idx } => {
            if disp.const_val.is_some() || disp.rpn.is_empty() {
                (ea.clone(), None)
            } else {
                match ctx.eval(&disp.rpn) {
                    Ok(v) => {
                        // displacement = target_addr - displacement_word_addr (8bit)
                        let target_addr = v.value;
                        let disp_word_addr = ctx.location() as i32 + 2;
                        let displacement = target_addr - disp_word_addr;
                        let new_disp = Displacement {
                            rpn: vec![RPNToken::Value(displacement as u32), RPNToken::End],
                            size: disp.size,
                            const_val: Some(displacement),
                        };
                        (EffectiveAddress::PcIdx { disp: new_disp, idx: idx.clone() }, None)
                    }
                    Err(name) => {
                        let new_disp = Displacement {
                            rpn: zero_rpn(),
                            size: disp.size,
                            const_val: Some(0),
                        };
                        (EffectiveAddress::PcIdx { disp: new_disp, idx: idx.clone() }, Some(EaExtKind::PcRel(name)))
                    }
                }
            }
        }
        other => (other.clone(), None),
    }
}

/// 命令サイズを考慮した EA 拡張バイト数（Immediate は .l のとき 4 バイト）
fn ea_ext_size_for_insn(ea: &EffectiveAddress, size: SizeCode) -> u32 {
    match ea {
        EffectiveAddress::Immediate(_) => match size {
            SizeCode::Long => 4,
            _ => 2,
        },
        other => ea_ext_size(other),
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
        EffectiveAddress::CcrReg | EffectiveAddress::SrReg
        | EffectiveAddress::FpReg(_) | EffectiveAddress::FpCtrlReg(_) => 0,
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
    suppressed: bool,
) {
    if suppressed {
        return;
    }
    let pc = ctx.location(); // 命令先頭のアドレス

    // ターゲットアドレスを評価
    let target_addr = match ctx.eval(target) {
        Ok(v) => v.value,
        Err(e) => {
            // 外部参照 → PC相対リロケーションレコードを生成
            let xref_num = ctx.get_or_add_xref(e);
            match req_size {
                Some(SizeCode::Long) => {
                    // .l 形式の外部参照: 未対応
                    ctx.advance(6);
                    ctx.num_errors += 1;
                }
                Some(SizeCode::Short) => {
                    // .s 形式: オペコードのみ出力、ディスプレースメントはリンカが提供
                    ctx.emit(&[(opcode >> 8) as u8, 0x00]);
                    // 命令長は2バイト。ディスプレースメントはオペコードに埋め込まれる（1バイト）
                    // loc = pc+1（ディスプレースメントバイトのアドレス）
                    let loc = pc + 1;
                    ctx.flush_code_buf();
                    ctx.flush_dsb();
                    let sect = ctx.cur_sect;
                    ctx.code_body.extend_from_slice(&[0x6B, sect]);
                    ctx.code_body.extend_from_slice(&loc.to_be_bytes());
                    ctx.code_body.push((xref_num >> 8) as u8);
                    ctx.code_body.push(xref_num as u8);
                }
                _ => {
                    // .w 形式 (デフォルト): オペコード2バイト + リンカ提供ディスプレースメント2バイト
                    ctx.emit(&[(opcode >> 8) as u8, 0x00]);
                    let loc = ctx.location(); // pc + 2 = ディスプレースメントスロットのアドレス
                    ctx.advance(2); // リンカが2バイトのディスプレースメントを提供
                    ctx.flush_code_buf();
                    ctx.flush_dsb();
                    let sect = ctx.cur_sect;
                    ctx.code_body.extend_from_slice(&[0x65, sect]);
                    ctx.code_body.extend_from_slice(&loc.to_be_bytes());
                    ctx.code_body.push((xref_num >> 8) as u8);
                    ctx.code_body.push(xref_num as u8);
                }
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::rpn::{Operator, RPNToken};

    #[test]
    fn test_is_external_with_offset_mul_add_const_fold() {
        let sym = SymbolTable::new(false);
        let rpn = vec![
            RPNToken::SymbolRef(b"extsym".to_vec()),
            RPNToken::Value(16),
            RPNToken::Value(4),
            RPNToken::Op(Operator::Mul),
            RPNToken::Op(Operator::Add),
            RPNToken::End,
        ];
        let got = is_external_with_offset(&rpn, &sym).expect("ext + 16*4");
        assert_eq!(got.0, b"extsym".to_vec());
        assert_eq!(got.1, 64);
    }
}
