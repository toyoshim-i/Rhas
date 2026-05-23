//! Pass 1: ソース行解析 → TempRecord 生成
//!
//! オリジナルの `main.s` の pass1 ルーチンに対応。
//! ソーステキストをスキャンし、シンボルを定義しながら TempRecord 列を構築する。

use crate::addressing::{parse_reg_list_mask, EffectiveAddress};
use crate::context::AssemblyContext;
use crate::error::{ErrorCode, SourcePos, warn, ErrorContext, WarnContext};
use crate::expr::{eval_rpn, parse_expr, Rpn};
use crate::expr::eval::EvalValue;
use crate::expr::rpn::RPNToken;
use crate::options::cpu as cpuconst;
use crate::source::{ReadResult, SourceStack};
use crate::symbol::{Symbol, SymbolTable};
use crate::symbol::types::{DefAttrib, ExtAttrib, FirstDef, InsnHandler, SizeCode};
use std::collections::HashMap;
use super::pseudo;
use super::temp::TempRecord;

mod preprocess;
mod operand;
mod insn;

use preprocess::{preprocess_anon_labels, preprocess_numeric_local_labels};
use operand::parse_operands;
use insn::handle_real_insn;

/// Pass1 の作業状態
pub struct P1Ctx<'a> {
    pub(super) sym:      &'a mut SymbolTable,
    pub(super) ctx:      &'a mut AssemblyContext,
    /// .if ネスト深度（最大 64 段）
    pub(super) if_nest:  u16,
    /// スキップ中の .if ネスト深度（0 = スキップしていない）
    pub(super) skip_nest: u16,
    /// スキップ中（is_if_skip）
    pub(super) is_skip:  bool,
    /// 各 if-nesting レベルでマッチ済みブランチがあるか（.elseif/.else の重複実行防止）
    pub(super) if_matched: [bool; 65],
    /// .end が来たか
    pub(super) is_end:   bool,
    /// ローカルラベルベース（マクロ展開番号用、将来実装）
    pub(super) local_base: u32,
    /// 匿名ローカルラベルカウンタ（@@: の通し番号）
    pub(super) local_anon_count: u32,
    /// 数値ローカルラベル（`1:` / `1f` / `1b`）の定義カウンタ
    pub(super) num_local_counts: HashMap<u32, u32>,
    /// 現在処理中のソース位置（エラーメッセージ用）
    pub(super) current_pos: SourcePos,
}

impl<'a> P1Ctx<'a> {
    pub(crate) fn new(sym: &'a mut SymbolTable, ctx: &'a mut AssemblyContext) -> Self {
        P1Ctx {
            sym, ctx,
            if_nest: 0,
            skip_nest: 0,
            is_skip: false,
            if_matched: [false; 65],
            is_end: false,
            local_base: 0,
            local_anon_count: 0,
            num_local_counts: HashMap::new(),
            current_pos: SourcePos::new(Vec::new(), 0),
        }
    }

    /// エラーを報告して count を増やす（error.rs テーブル経由）
    pub(super) fn error_code(&mut self, code: ErrorCode, sym: Option<&[u8]>) {
        let err_ctx = match sym {
            Some(s) => ErrorContext::with_symbol(self.current_pos.clone(), code, s),
            None => ErrorContext::new(self.current_pos.clone(), code, None),
        };
        let mut stderr = std::io::stderr();
        crate::error::print_error_context(&mut stderr, &err_ctx);
        self.ctx.add_error();
    }

    pub(super) fn warn_code(&mut self, code: crate::error::WarnCode, sym: Option<&[u8]>) {
        let level = self.ctx.effective_warn_level();
        let warn_ctx = match sym {
            Some(s) => WarnContext::with_symbol(self.current_pos.clone(), code, s),
            None => WarnContext::new(self.current_pos.clone(), code, None),
        };
        let mut stderr = std::io::stderr();
        crate::error::print_warning_context(&mut stderr, &warn_ctx, level);
        if level >= crate::error::warn_default_level(code) {
            self.ctx.add_warning();
        }
    }

    pub(super) fn section_id(&self) -> u8 {
        if self.ctx.is_offset_mode { 0 } else { self.ctx.section as u8 }
    }
    pub(super) fn is_offset_mode(&self) -> bool { self.ctx.is_offset_mode }

    /// Consume and increment the local base, returning the previous value.
    pub(super) fn next_local_base(&mut self) -> u32 {
        let v = self.local_base;
        self.local_base = self.local_base.wrapping_add(1);
        v
    }
    pub(super) fn cpu_type(&self)   -> u16 { self.ctx.cpu.features }
    pub(super) fn location(&self)   -> u32 { self.ctx.location() }

    pub(super) fn advance(&mut self, n: u32) {
        self.ctx.advance_location(n);
    }

    /// シンボル定義（ロケーションラベル）
    pub(super) fn define_label(&mut self, name: Vec<u8>, section: u8, offset: u32) {
        let sym = Symbol::Value {
            attrib:     DefAttrib::Define,
            ext_attrib: ExtAttrib::None,
            section,
            org_num:    0,
            first:      FirstDef::Other,
            opt_count:  0,
            value:      offset as i32,
        };
        self.sym.define(name, sym);
    }

    /// RPN 式を定数評価する
    pub(super) fn eval_const(&self, rpn: &Rpn) -> Option<EvalValue> {
        let loc = self.ctx.loc_top;
        let cur = self.location();
        let sec = self.section_id();
        let result = eval_rpn(rpn, loc, cur, sec, &|name| {
            self.sym.lookup_sym(name).and_then(sym_to_eval)
        });
        result.ok()
    }
}

/// Symbol → EvalValue 変換
fn sym_to_eval(sym: &Symbol) -> Option<EvalValue> {
    if let Symbol::Value { value, section, attrib, .. } = sym {
        if *attrib >= DefAttrib::Define {
            return Some(EvalValue { value: *value, section: *section });
        }
    }
    None
}

/// Pass1: ソース → TempRecord 列
pub fn pass1(
    source: &mut SourceStack,
    ctx:    &mut AssemblyContext,
    sym:    &mut SymbolTable,
) -> Vec<TempRecord> {
    let mut records: Vec<TempRecord> = Vec::with_capacity(4096);
    let mut p1 = P1Ctx::new(sym, ctx);

    loop {
        match source.read_line() {
            ReadResult::Eof => break,
            ReadResult::IncludeEnd => {
                // HAS はインクルード終了時にセクション変更レコードを出力しない
            }
            ReadResult::Line(line) => {
                if should_emit_line_info(&line, &p1, false) {
                    let line_num = source.current().line;
                    records.push(TempRecord::LineInfo { line_num, text: line.clone(), is_macro: false });
                }
                // HAS互換: -g 時は .text の各ソース行開始で行番号データを記録する。
                // .include 内・空行は除外する。コメント行は含める。
                if p1.ctx.opts.make_sym_deb
                    && source.nest_depth() == 1
                    && p1.section_id() == 1
                    && !line.is_empty()
                {
                    let line_u16 = source.current().line as u16;
                    records.push(TempRecord::ScdAutoLn {
                        line: line_u16,
                        loc: vec![RPNToken::Location, RPNToken::End],
                    });
                }
                parse_line(&line, &mut records, &mut p1, source);
                if p1.is_end { break; }
            }
        }
    }

    records
}

/// 行を先読みし、`.list/.nlist` の PRN 制御擬似命令かどうか判定する。
/// 戻り値: `Some(true)=.list`, `Some(false)=.nlist`, `None=その他`
fn detect_prn_list_control(line: &[u8], p1: &P1Ctx<'_>) -> Option<bool> {
    if line.is_empty() { return None; }
    if line.first() == Some(&b'*') || line.first() == Some(&b';') { return None; }

    let mut pos = 0usize;
    if line[0] != b' ' && line[0] != b'\t' {
        let _ = parse_label(line, &mut pos);
    }
    skip_spaces(line, &mut pos);
    if pos >= line.len() || line[pos] == b';' {
        return None;
    }

    let (mnem, _) = parse_mnemonic(line, &mut pos);
    if mnem.is_empty() {
        return None;
    }

    let handler = p1.sym.lookup_cmd(&mnem, p1.cpu_type())
        .and_then(|s| {
            if let Symbol::Opcode { handler, .. } = s {
                Some(*handler)
            } else {
                None
            }
        });
    match handler {
        Some(InsnHandler::List) => Some(true),
        Some(InsnHandler::Nlist) => Some(false),
        _ => None,
    }
}

/// 現在の設定で行情報を PRN に出力するかを判定する。
fn should_emit_line_info(line: &[u8], p1: &P1Ctx<'_>, is_macro: bool) -> bool {
    if !p1.ctx.opts.make_prn || !p1.ctx.prn_listing {
        return false;
    }
    if is_macro && !p1.ctx.prn_macro_listing {
        return false;
    }
    // `.nlist` は当該行から listing を停止する。
    if matches!(detect_prn_list_control(line, p1), Some(false)) {
        return false;
    }
    true
}

fn parse_line(
    line: &[u8],
    records: &mut Vec<TempRecord>,
    p1: &mut P1Ctx<'_>,
    source: &mut SourceStack,
) {
    // `*` (RPNToken::Location) は「行頭ロケーション」を参照するため、
    // 各行解析の開始時点で loc_top を現在ロケーションに同期する。
    p1.ctx.loc_top = p1.location();

    // 匿名ローカルラベル（@@: / @b / @f）の事前展開
    let (processed_buf, is_anon_def) = preprocess_anon_labels(line, p1.local_anon_count);
    if is_anon_def {
        p1.local_anon_count += 1;
    }
    let processed_buf = preprocess_numeric_local_labels(&processed_buf, &mut p1.num_local_counts);
    let line: &[u8] = &processed_buf;

    let mut pos = 0;

    // 現在のソース位置を更新（エラーメッセージ用）
    p1.current_pos = source.source_pos();
    records.push(TempRecord::PositionMarker(p1.current_pos.clone()));

    // 行頭の '*' → コメント行
    if line.first() == Some(&b'*') { return; }

    // 行頭の ';' → コメント行
    if line.first() == Some(&b';') { return; }

    // 空行
    if line.is_empty() { return; }

    // ラベル解析（行頭が非空白）
    let (label, is_global_label) = if line[0] != b' ' && line[0] != b'\t' {
        match parse_label(line, &mut pos) {
            Some((name, is_global)) => (Some(name), is_global),
            None => (None, false),
        }
    } else {
        (None, false)
    };

    // 空白スキップ
    skip_spaces(line, &mut pos);
    if pos >= line.len() || line[pos] == b';' {
        // ラベルだけの行
        if let Some(ref name) = label {
            if !p1.is_skip {
                let sec = p1.section_id();
                let off = p1.location();
                p1.define_label(name.clone(), sec, off);
                records.push(TempRecord::LabelDef {
                    name: name.clone(), section: sec, offset: off
                });
                // '::' グローバルラベル → export
                if is_global_label {
                    records.push(TempRecord::XDef { name: name.clone() });
                    // ext_attrib を更新（try_register_xdef で早期検出できるように）
                    if let Some(Symbol::Value { ext_attrib, .. }) = p1.sym.lookup_sym_mut(name) {
                        *ext_attrib = ExtAttrib::XDef;
                    }
                }
            }
        }
        return;
    }

    // Case 1: 行頭ラベル後の ':=' → SET（例: N:=7）
    // parse_label が ':' を消費した後、次が '=' の場合
    if let Some(ref lbl) = label {
        if pos < line.len() && line[pos] == b'=' {
            if !p1.is_skip {
                pos += 1; // '=' を消費
                skip_spaces(line, &mut pos);
                handle_set_assignment(lbl, line, &mut pos, p1);
            }
            return;
        }
    }

    // ニーモニック + サイズ解析
    let word_start = pos;
    let (mnem, size) = parse_mnemonic(line, &mut pos);
    if mnem.is_empty() { return; }

    // Case 2: インデントされた行での word:=expr パターン（例: \tN:=7）
    // word: 後に '=' が続く場合のみ処理（generic label: insn は行頭非空白で処理）
    if label.is_none() && pos < line.len() && line[pos] == b':' {
        let next = pos + 1;
        if next < line.len() && line[next] == b'=' {
            // word:=expr → SET
            let name = line[word_start..pos].to_vec();
            pos += 2; // ':=' を消費
            skip_spaces(line, &mut pos);
            if !p1.is_skip {
                handle_set_assignment(&name, line, &mut pos, p1);
            }
            return;
        }
    }

    // スキップ中は .if 系のみ処理
    if p1.is_skip {
        let h = p1.sym.lookup_cmd(&mnem, p1.cpu_type())
            .and_then(|s| if let Symbol::Opcode { handler, .. } = s { Some(*handler) } else { None });
        pseudo::conditional::handle_skip(h, line, &mut pos, p1);
        return;
    }

    // ラベルが .equ / .set 以外 → ロケーションラベルとして登録
    // .equ / .set の場合はシンボルを後で登録する
    enum Dispatch {
        Pseudo(InsnHandler),
        RealInsn(InsnHandler, u16),
        MacroCall,
        Unknown,
    }
    let dispatch = match p1.sym.lookup_cmd(&mnem, p1.cpu_type()) {
        Some(Symbol::Opcode { handler, opcode, arch, .. }) => {
            if arch.is_pseudo() {
                Dispatch::Pseudo(*handler)
            } else {
                Dispatch::RealInsn(*handler, *opcode)
            }
        }
        Some(Symbol::Macro { .. }) => Dispatch::MacroCall,
        _ => Dispatch::Unknown,
    };

    let is_equ = matches!(dispatch, Dispatch::Pseudo(InsnHandler::Equ | InsnHandler::Set | InsnHandler::Reg));

    // ロケーションラベルを先に登録（.equ/.set 以外）
    // XDef TempRecord は命令処理後に追加する（HAS互換の順序: XREF → XDEF）
    if !is_equ {
        if let Some(ref name) = label {
            let sec = p1.section_id();
            let off = p1.location();
            p1.define_label(name.clone(), sec, off);
            records.push(TempRecord::LabelDef {
                name: name.clone(), section: sec, offset: off
            });
        }
    }

    // 操作列の残り（オペランド部）
    skip_spaces(line, &mut pos);

    // ニーモニックのディスパッチ
    match dispatch {
        // ---- 疑似命令 ----
        Dispatch::Pseudo(handler) => {
            handle_pseudo(
                handler, &mnem, size, line, &mut pos, &label,
                records, p1, source
            );
        }
        // ---- 実命令 ----
        Dispatch::RealInsn(handler, opcode) => {
            handle_real_insn(
                handler, opcode, size, line, pos, records, p1
            );
        }
        // ---- マクロ呼び出し ----
        Dispatch::MacroCall => {
            if let Some(Symbol::Macro { params, local_count: _, template }) =
                p1.sym.lookup_cmd(&mnem, p1.cpu_type()).cloned()
            {
                let args = parse_macro_args(line, &mut pos);
                let lb = p1.next_local_base();
                expand_macro_body(&template, &params, &args, lb, records, p1, source);
            }
        }
        // ---- 未知のニーモニック ----
        Dispatch::Unknown => {
            p1.error_code(ErrorCode::BadOpe, None);
        }
    }

    // '::' グローバルラベル → 命令の後に XDEF を追加（HAS互換の順序: 命令XREF → ラベルXDEF）
    if !is_equ && is_global_label {
        if let Some(ref name) = label {
            records.push(TempRecord::XDef { name: name.clone() });
            // ext_attrib を更新（try_register_xdef で早期検出できるように）
            if let Some(Symbol::Value { ext_attrib, .. }) = p1.sym.lookup_sym_mut(name) {
                *ext_attrib = ExtAttrib::XDef;
            }
        }
    }
}

/// `name := expr` 形式の代入を処理する（.set と同等）
fn handle_set_assignment(name: &[u8], line: &[u8], pos: &mut usize, p1: &mut P1Ctx<'_>) {
    if let Ok(rpn) = parse_expr(line, pos) {
        if let Some(v) = p1.eval_const(&rpn) {
            let sym = Symbol::Value {
                attrib:     DefAttrib::Define,
                ext_attrib: ExtAttrib::None,
                section:    v.section,
                org_num:    0,
                first:      FirstDef::Other,
                opt_count:  0,
                value:      v.value,
            };
            p1.sym.define(name.to_vec(), sym);
        }
    }
}

/// (name, is_global) を返す
fn parse_label(line: &[u8], pos: &mut usize) -> Option<(Vec<u8>, bool)> {
    let start = *pos;
    // '.' で始まる場合は疑似命令 → ラベルではない
    if line.get(start) == Some(&b'.') { return None; }
    let mut end = start;
    while end < line.len() {
        let b = line[end];
        if b == b':' || b == b' ' || b == b'\t' || b == b';' { break; }
        end += 1;
    }
    if end == start { return None; }
    let name = line[start..end].to_vec();
    *pos = end;
    // ':' があれば消費する
    let mut is_global = false;
    if *pos < line.len() && line[*pos] == b':' {
        *pos += 1;
        // '::' (double colon) = グローバルラベル
        if *pos < line.len() && line[*pos] == b':' {
            *pos += 1;
            is_global = true;
        }
    }
    Some((name, is_global))
}

fn parse_mnemonic(line: &[u8], pos: &mut usize) -> (Vec<u8>, Option<SizeCode>) {
    // 行頭の '.' を消費（疑似命令のプレフィックス）
    let has_dot = *pos < line.len() && line[*pos] == b'.';
    if has_dot { *pos += 1; }

    // ニーモニック本体
    let start = *pos;
    while *pos < line.len() && is_mnem_char(line[*pos]) {
        *pos += 1;
    }
    let mnem_raw = &line[start..*pos];
    if mnem_raw.is_empty() { return (Vec::new(), None); }

    // サイズサフィックス (.b / .w / .l / .s / .d / .x / .p)
    let size = if *pos < line.len() && line[*pos] == b'.' {
        let sz_pos = *pos + 1;
        if let Some(s) = sz_pos.checked_sub(0).and_then(|_| line.get(sz_pos)) {
            let parsed = match s {
                b'b' | b'B' => Some(SizeCode::Byte),
                b'w' | b'W' => Some(SizeCode::Word),
                b'l' | b'L' => Some(SizeCode::Long),
                b's' | b'S' => Some(SizeCode::Short),
                b'd' | b'D' => Some(SizeCode::Double),
                b'x' | b'X' => Some(SizeCode::Extend),
                b'p' | b'P' => Some(SizeCode::Packed),
                _ => None,
            };
            if let Some(sz) = parsed {
                // サイズ文字の次が非アルファベット（または EOF）ならサイズサフィックス確定
                let after = sz_pos + 1;
                if after >= line.len() || !line[after].is_ascii_alphanumeric() {
                    *pos = after;
                    Some(sz)
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    let mnem = crate::utils::to_lowercase_vec(mnem_raw);
    (mnem, size)
}

fn is_mnem_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

pub(super) fn skip_spaces(line: &[u8], pos: &mut usize) {
    while *pos < line.len() && matches!(line[*pos], b' ' | b'\t') {
        *pos += 1;
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_pseudo(
    handler:  InsnHandler,
    mnem:     &[u8],
    size:     Option<SizeCode>,
    line:     &[u8],
    pos:      &mut usize,
    label:    &Option<Vec<u8>>,
    records:  &mut Vec<TempRecord>,
    p1:       &mut P1Ctx<'_>,
    source:   &mut SourceStack,
) {
    let _ = mnem;
    match handler {
        // ---- セクション切り替え ----
        InsnHandler::TextSect | InsnHandler::DataSect | InsnHandler::BssSect |
        InsnHandler::Stack | InsnHandler::RdataSect | InsnHandler::RbssSect |
        InsnHandler::RstackSect | InsnHandler::RldataSect | InsnHandler::RlbssSect |
        InsnHandler::RlstackSect => {
            pseudo::section::handle_section(handler, p1.ctx, records);
        }

        // ---- .offset / .even / .quad / .align ----
        InsnHandler::Offset | InsnHandler::Even | InsnHandler::Quad | InsnHandler::Align => {
            pseudo::misc::handle_misc(handler, &label, line, pos, p1, records);
        }

        // ---- .dc/.ds/.dcb ----
        InsnHandler::Dc | InsnHandler::Ds | InsnHandler::Dcb => {
            pseudo::data::handle_data(handler, size, line, pos, p1, records, source);
        }

        // ---- .equ / .set ----
        InsnHandler::Equ | InsnHandler::Set => {
            skip_spaces(line, pos);
            if let Some(ref name) = label {
                if let Ok(rpn) = parse_expr(line, pos) {
                    records.push(TempRecord::EquDef { name: name.clone(), rpn: rpn.clone() });
                    if let Some(v) = p1.eval_const(&rpn) {
                        let attrib = match handler {
                            // .set は時系列値としてその時点で確定させる
                            InsnHandler::Set => DefAttrib::Define,
                            // .equ はロケーション依存式なら後段で再評価
                            _ => {
                                if is_dynamic_equ_expr(&rpn, p1.sym) {
                                    DefAttrib::NoDet
                                } else {
                                    DefAttrib::Define
                                }
                            }
                        };
                        let sym = Symbol::Value {
                            attrib,
                            ext_attrib: ExtAttrib::None,
                            section:    v.section,
                            org_num:    0,
                            first:      FirstDef::Other,
                            opt_count:  0,
                            value:      v.value,
                        };
                        p1.sym.define(name.clone(), sym);
                    } else {
                        let sym = Symbol::Value {
                            attrib:     DefAttrib::NoDet,
                            ext_attrib: ExtAttrib::None,
                            section:    0,
                            org_num:    0,
                            first:      FirstDef::Other,
                            opt_count:  0,
                            value:      0,
                        };
                        p1.sym.define(name.clone(), sym);
                    }
                }
            }
        }

        // ---- .xdef ----
        InsnHandler::Xdef => {
            if let Some(ref name) = label {
                records.push(TempRecord::XDef { name: name.clone() });
                if let Some(Symbol::Value { ext_attrib, .. }) = p1.sym.lookup_sym_mut(name) {
                    *ext_attrib = ExtAttrib::XDef;
                }
            }
            skip_spaces(line, pos);
            while *pos < line.len() && line[*pos] != b';' {
                let name = read_ident(line, pos);
                if name.is_empty() { break; }
                records.push(TempRecord::XDef { name: name.clone() });
                if let Some(Symbol::Value { ext_attrib, .. }) = p1.sym.lookup_sym_mut(&name) {
                    *ext_attrib = ExtAttrib::XDef;
                }
                skip_spaces(line, pos);
                if *pos < line.len() && line[*pos] == b',' {
                    *pos += 1;
                    skip_spaces(line, pos);
                } else { break; }
            }
        }

        // ---- .xref ----
        InsnHandler::Xref => {
            pseudo::misc::handle_misc(handler, &label, line, pos, p1, records);
        }

        // ---- .if / .ifdef / .ifndef / .else / .elseif / .endif ----
        InsnHandler::If | InsnHandler::Iff | InsnHandler::Ifdef | InsnHandler::Ifndef
        | InsnHandler::Else | InsnHandler::Elseif | InsnHandler::Endif => {
            pseudo::conditional::handle_conditional(handler, line, pos, p1);
        }

        // ---- .include ----
        InsnHandler::Include | InsnHandler::Insert => {
            skip_spaces(line, pos);
            let fname = parse_filename(line, pos);
            if !fname.is_empty() {
                let _ = source.push_include(&fname);
            }
        }

        // ---- .request ----
        InsnHandler::Request => {
            skip_spaces(line, pos);
            let fname = parse_filename(line, pos);
            if !fname.is_empty() {
                p1.ctx.request_files.push(fname);
            }
        }

        // ---- .end ----
        InsnHandler::End => {
            records.push(TempRecord::End);
            p1.is_end = true;
        }

        // ---- .cpu / CPU 指定 ----
        InsnHandler::Cpu
        | InsnHandler::Cpu68000 | InsnHandler::Cpu68010 | InsnHandler::Cpu68020
        | InsnHandler::Cpu68030 | InsnHandler::Cpu68040 | InsnHandler::Cpu68060
        | InsnHandler::Cpu5200 | InsnHandler::Cpu5300 | InsnHandler::Cpu5400 => {
            pseudo::misc::handle_misc(handler, &label, line, pos, p1, records);
        }

        // ---- リスト制御 ----
        InsnHandler::List => {
            p1.ctx.prn_listing = true;
        }
        InsnHandler::Nlist => {
            p1.ctx.prn_listing = false;
        }
        InsnHandler::Lall => {
            p1.ctx.prn_macro_listing = true;
        }
        InsnHandler::Sall => {
            p1.ctx.prn_macro_listing = false;
        }
        InsnHandler::Width => {
            skip_spaces(line, pos);
            match parse_expr(line, pos).ok().and_then(|rpn| p1.eval_const(&rpn).map(|v| v.value)) {
                Some(v) if (80..=255).contains(&v) => {
                    p1.ctx.opts.prn_width = (v as u16) & !7;
                }
                _ => {
                    p1.error_code(ErrorCode::IlValue, None);
                }
            }
        }
        InsnHandler::Page => {
            skip_spaces(line, pos);
            if *pos >= line.len() || line[*pos] == b';' {
                // 改ページのみ（値変更なし）
            } else if line[*pos] == b'+' {
                // `.page +`（値変更なし）
            } else {
                match parse_expr(line, pos).ok().and_then(|rpn| p1.eval_const(&rpn).map(|v| v.value)) {
                    Some(v) if v < 0 => {
                        p1.ctx.opts.prn_page_lines = u16::MAX;
                    }
                    Some(v) if (10..=255).contains(&v) => {
                        p1.ctx.opts.prn_page_lines = v as u16;
                    }
                    _ => {
                        p1.error_code(ErrorCode::IlValue, None);
                    }
                }
            }
        }
        InsnHandler::Title => {
            p1.ctx.prn_title = parse_prn_text(line, pos);
        }
        InsnHandler::SubTtl => {
            p1.ctx.prn_subttl = parse_prn_text(line, pos);
        }

        // ---- .fail ----
        InsnHandler::Fail => {
            pseudo::misc::handle_misc(handler, &label, line, pos, p1, records);
        }

        // ---- macro-style pseudos ----
        InsnHandler::MacroDef | InsnHandler::Rept | InsnHandler::Irp | InsnHandler::Irpc => {
            pseudo::macro_::handle_macro(handler, label.clone(), line, pos, source, p1, records);
        }

        // ---- .endm / .exitm / .local / .sizem ----
        InsnHandler::EndM | InsnHandler::ExitM | InsnHandler::Local | InsnHandler::SizeM => {}

        // ---- SCD デバッグ ----
        InsnHandler::FileScd | InsnHandler::Def | InsnHandler::Endef | InsnHandler::Val | InsnHandler::Scl
        | InsnHandler::TypeScd | InsnHandler::Tag | InsnHandler::Ln | InsnHandler::Line
        | InsnHandler::SizeScd | InsnHandler::Dim => {
            pseudo::debug::handle_scd(handler, line, pos, p1, records);
        }

        // ---- .reg ----
        InsnHandler::Reg => {
            skip_spaces(line, pos);
            if let Some(ref name) = label {
                let saved_pos = *pos;
                let reg_mask = parse_reg_list_mask(line, pos, p1.sym, p1.ctx.cpu.features);
                let rpns: Vec<Rpn> = if let Some(mask) = reg_mask {
                    vec![vec![RPNToken::ValueWord(mask), RPNToken::End]]
                } else {
                    *pos = saved_pos;
                    let mut list: Vec<Rpn> = Vec::new();
                    loop {
                        if *pos >= line.len() || line[*pos] == b';' { break; }
                        match parse_expr(line, pos) {
                            Ok(rpn) => list.push(rpn),
                            Err(_) => break,
                        }
                        skip_spaces(line, pos);
                        if *pos < line.len() && line[*pos] == b',' {
                            *pos += 1;
                            skip_spaces(line, pos);
                        } else {
                            break;
                        }
                    }
                    if list.len() == 1 {
                        if let [RPNToken::SymbolRef(target), RPNToken::End] = list[0].as_slice() {
                            let target = target.clone();
                            if p1.sym.lookup_sym(&target).is_none() {
                                let sym = Symbol::Value {
                                    attrib:     DefAttrib::Undef,
                                    ext_attrib: ExtAttrib::XRef,
                                    section:    0xFF,
                                    org_num:    0,
                                    first:      FirstDef::Other,
                                    opt_count:  0,
                                    value:      0,
                                };
                                p1.sym.define(target.clone(), sym);
                            }
                            records.push(TempRecord::XRef { name: target });
                        }
                    }
                    list
                };
                p1.sym.define(name.clone(), Symbol::RegSym { define: rpns });
            }
        }

        // ---- .comm / .rcomm / .rlcomm ----
        InsnHandler::Comm | InsnHandler::Rcomm | InsnHandler::Rlcomm => {
            pseudo::misc::handle_misc(handler, &label, line, pos, p1, records);
        }

        // ---- .offsym ----
        InsnHandler::OffsymPs => {
            skip_spaces(line, pos);
            let init = if *pos < line.len() {
                if let Ok(rpn) = parse_expr(line, pos) {
                    p1.eval_const(&rpn).map(|v| v.value).unwrap_or(0)
                } else {
                    p1.error_code(ErrorCode::Expr, None);
                    return;
                }
            } else {
                p1.error_code(ErrorCode::IlOpr, None);
                return;
            };

            skip_spaces(line, pos);
            let mut has_symbol = false;
            if *pos < line.len() && line[*pos] == b',' {
                *pos += 1;
                skip_spaces(line, pos);
                let name = read_ident(line, pos);
                if name.is_empty() {
                    p1.error_code(ErrorCode::NoSymPseudo, Some(b".offsym"));
                    return;
                }
                let mut warn_overwrite = false;
                match p1.sym.lookup_sym_mut(&name) {
                    Some(Symbol::Value { attrib, section, first, value, ext_attrib, .. }) => {
                        if *first != FirstDef::Offsym && *attrib >= DefAttrib::Define {
                            if p1.ctx.opts.ow_offsym {
                                p1.error_code(ErrorCode::RedefOffsym, Some(&name));
                                return;
                            }
                            warn_overwrite = true;
                        }
                        *attrib = DefAttrib::Define;
                        *ext_attrib = ExtAttrib::None;
                        *section = 0;
                        *first = FirstDef::Offsym;
                        *value = init;
                    }
                    Some(_) => {
                        p1.error_code(ErrorCode::IlSymValue, None);
                        return;
                    }
                    None => {
                        let sym = Symbol::Value {
                            attrib:     DefAttrib::Define,
                            ext_attrib: ExtAttrib::None,
                            section:    0,
                            org_num:    0,
                            first:      FirstDef::Offsym,
                            opt_count:  0,
                            value:      init,
                        };
                        p1.sym.define(name.clone(), sym);
                    }
                }
                if warn_overwrite {
                    p1.warn_code(warn::REDEF_OFFSYM, Some(&name));
                }
                has_symbol = true;
                skip_spaces(line, pos);
            }

            if *pos < line.len() && line[*pos] != b';' {
                p1.error_code(ErrorCode::IlOpr, None);
                return;
            }
            p1.ctx.offsym_with_symbol = has_symbol;
            p1.ctx.set_offset_mode(init as u32);
        }

        // ---- FP 等 ----
        InsnHandler::FpId => {
            skip_spaces(line, pos);
            if *pos >= line.len() || line[*pos] == b';' {
                p1.error_code(ErrorCode::IlOpr, None);
                return;
            }
            let value = match parse_expr(line, pos).ok().and_then(|rpn| p1.eval_const(&rpn)) {
                Some(v) if v.section == 0 => v.value,
                _ => {
                    p1.error_code(ErrorCode::Expr, None);
                    return;
                }
            };
            skip_spaces(line, pos);
            if *pos < line.len() && line[*pos] != b';' {
                p1.error_code(ErrorCode::IlOpr, None);
                return;
            }
            if value < 0 {
                p1.ctx.cpu.features &= !cpuconst::CFPP;
            } else if value <= 7 {
                p1.ctx.fpid = value as u8;
            } else {
                p1.error_code(ErrorCode::IlValue, None);
            }
        }
        InsnHandler::Pragma => {}

        _ => {}
    }
}

fn is_dynamic_equ_expr(rpn: &Rpn, sym: &SymbolTable) -> bool {
    for tok in rpn {
        match tok {
            RPNToken::Location | RPNToken::CurrentLoc => return true,
            RPNToken::SymbolRef(name) => {
                match sym.lookup_sym(name) {
                    Some(Symbol::Value { section, attrib, .. }) => {
                        if *attrib < DefAttrib::Define || *section != 0 {
                            return true;
                        }
                    }
                    _ => return true,
                }
            }
            _ => {}
        }
    }
    false
}

pub(crate) fn parse_align_n(line: &[u8], pos: &mut usize, p1: &mut P1Ctx<'_>) -> Option<u8> {
    if let Ok(rpn) = parse_expr(line, pos) {
        if let Some(v) = p1.eval_const(&rpn) {
            let align = v.value as u32;
            if align >= 2 {
                let mut n = 0u8;
                let mut a = align;
                while a > 1 { a >>= 1; n += 1; }
                return Some(n);
            }
        }
    }
    None
}

pub(crate) fn parse_align_pad(line: &[u8], pos: &mut usize, p1: &mut P1Ctx<'_>) -> Option<u16> {
    skip_spaces(line, pos);
    if *pos < line.len() && line[*pos] == b',' {
        *pos += 1;
        skip_spaces(line, pos);
        if let Ok(rpn) = parse_expr(line, pos) {
            if let Some(v) = p1.eval_const(&rpn) {
                return Some(v.value as u16);
            }
        }
    }
    None
}

pub(crate) fn read_ident(line: &[u8], pos: &mut usize) -> Vec<u8> {
    let start = *pos;
    while *pos < line.len() {
        let b = line[*pos];
        if b.is_ascii_alphanumeric() || b == b'_' || b == b'.' || b == b'@' {
            *pos += 1;
        } else {
            break;
        }
    }
    line[start..*pos].to_vec()
}

pub(crate) fn parse_string_or_ident(line: &[u8], pos: &mut usize) -> Vec<u8> {
    if *pos >= line.len() { return Vec::new(); }
    if line[*pos] == b'"' || line[*pos] == b'\'' {
        let quote = line[*pos];
        *pos += 1;
        let start = *pos;
        while *pos < line.len() && line[*pos] != quote { *pos += 1; }
        let s = line[start..*pos].to_vec();
        if *pos < line.len() { *pos += 1; }
        s
    } else {
        read_ident(line, pos)
    }
}

pub(crate) fn parse_filename(line: &[u8], pos: &mut usize) -> Vec<u8> {
    if *pos >= line.len() { return Vec::new(); }
    if line[*pos] == b'"' || line[*pos] == b'\'' {
        let quote = line[*pos];
        *pos += 1;
        let start = *pos;
        while *pos < line.len() && line[*pos] != quote { *pos += 1; }
        let s = line[start..*pos].to_vec();
        if *pos < line.len() { *pos += 1; }
        s
    } else {
        let start = *pos;
        while *pos < line.len() {
            let b = line[*pos];
            if b == b' ' || b == b'\t' || b == b';' { break; }
            *pos += 1;
        }
        line[start..*pos].to_vec()
    }
}

fn parse_prn_text(line: &[u8], pos: &mut usize) -> Vec<u8> {
    skip_spaces(line, pos);
    if *pos >= line.len() {
        return Vec::new();
    }

    let mut s = if line[*pos] == b'"' || line[*pos] == b'\'' {
        parse_string_or_ident(line, pos)
    } else {
        let start = *pos;
        while *pos < line.len() && line[*pos] != b';' {
            *pos += 1;
        }
        line[start..*pos].to_vec()
    };

    while let Some(&b) = s.last() {
        if b == b' ' || b == b'\t' {
            s.pop();
        } else {
            break;
        }
    }
    s
}

pub(crate) fn parse_macro_params(line: &[u8], pos: &mut usize) -> Vec<Vec<u8>> {
    let mut params = Vec::new();
    skip_spaces(line, pos);
    while *pos < line.len() && line[*pos] != b';' && line[*pos] != b'*' {
        let p = read_ident(line, pos);
        if p.is_empty() { break; }
        params.push(p);
        skip_spaces(line, pos);
        if *pos < line.len() && line[*pos] == b',' {
            *pos += 1;
            skip_spaces(line, pos);
        } else {
            break;
        }
    }
    params
}

pub(crate) fn parse_macro_args(line: &[u8], pos: &mut usize) -> Vec<Vec<u8>> {
    let mut args = Vec::new();
    skip_spaces(line, pos);
    while *pos < line.len() && line[*pos] != b';' && line[*pos] != b'*' {
        let arg = parse_one_macro_arg(line, pos);
        args.push(arg);
        skip_spaces(line, pos);
        if *pos < line.len() && line[*pos] == b',' {
            *pos += 1;
            skip_spaces(line, pos);
        } else {
            break;
        }
    }
    args
}

fn parse_one_macro_arg(line: &[u8], pos: &mut usize) -> Vec<u8> {
    if *pos >= line.len() { return Vec::new(); }
    if line[*pos] == b'<' {
        *pos += 1;
        let mut arg = Vec::new();
        let mut nest = 1u32;
        while *pos < line.len() {
            let b = line[*pos];
            *pos += 1;
            if b == b'<' { nest += 1; arg.push(b); }
            else if b == b'>' {
                nest -= 1;
                if nest == 0 { break; }
                arg.push(b);
            } else {
                arg.push(b);
            }
        }
        arg
    } else {
        let start = *pos;
        while *pos < line.len() {
            let b = line[*pos];
            if b == b',' || b == b';' || b == b'\n' { break; }
            *pos += 1;
        }
        let end = *pos;
        let s = &line[start..end];
        s.iter().rev().skip_while(|&&b| b == b' ' || b == b'\t').count();
        let trim_end = end - s.iter().rev().take_while(|&&b| b == b' ' || b == b'\t').count();
        line[start..trim_end].to_vec()
    }
}

pub(crate) fn collect_macro_body(
    source: &mut SourceStack,
    sym:    &SymbolTable,
    ctx:    &mut AssemblyContext,
    params: &[Vec<u8>],
) -> (Vec<u8>, u16) {
    let mut template = Vec::new();
    let mut local_count = 0u16;
    let mut nest_depth = 0u32;
    let mut name_map: std::collections::HashMap<Vec<u8>, u16> = std::collections::HashMap::new();

    while let ReadResult::Line(line) = source.read_line() {
        let trim_len = line.iter().rev().take_while(|&&b| b == b'\r' || b == b'\n').count();
        let line = &line[..line.len() - trim_len];

        let mnem = extract_mnemonic(line);
        let handler = sym.lookup_cmd(&mnem, ctx.cpu.features)
            .and_then(|s| if let Symbol::Opcode { handler, .. } = s { Some(*handler) } else { None });

        match handler {
            Some(InsnHandler::MacroDef | InsnHandler::Rept | InsnHandler::Irp | InsnHandler::Irpc) => {
                nest_depth += 1;
                template.extend_from_slice(line);
                template.push(b'\n');
            }
            Some(InsnHandler::EndM) => {
                if nest_depth == 0 {
                    break;
                }
                nest_depth -= 1;
                template.extend_from_slice(line);
                template.push(b'\n');
            }
            _ => {
                if nest_depth == 0 {
                    let converted = convert_line_params(line, params, &mut local_count, &mut name_map);
                    template.extend_from_slice(&converted);
                } else {
                    template.extend_from_slice(line);
                }
                template.push(b'\n');
            }
        }
    }

    (template, local_count)
}

fn convert_line_params(
    line: &[u8],
    params: &[Vec<u8>],
    local_count: &mut u16,
    name_map: &mut std::collections::HashMap<Vec<u8>, u16>,
) -> Vec<u8> {
    let mut out = Vec::with_capacity(line.len() + 8);
    let mut i = 0;
    while i < line.len() {
        let b = line[i];
        if b == b';' {
            out.extend_from_slice(&line[i..]);
            break;
        }
        if b == b'&' {
            i += 1;
            if i < line.len() && line[i] == b'&' {
                out.push(b'&');
                i += 1;
                continue;
            }
            let start = i;
            while i < line.len() && (line[i].is_ascii_alphanumeric() || line[i] == b'_') {
                i += 1;
            }
            let name = &line[start..i];
            if let Some(idx) = params.iter().position(|p| {
                p.len() == name.len() && p.iter().zip(name).all(|(a,b)| a.eq_ignore_ascii_case(b))
            }) {
                out.push(0xFF);
                out.push((idx >> 8) as u8);
                out.push((idx & 0xFF) as u8);
            } else {
                out.push(b'&');
                out.extend_from_slice(name);
            }
            continue;
        }
        if b == b'@' && i + 1 < line.len() && line[i+1] != b'@' {
            let next = line[i + 1];
            let after = i + 2;
            let is_anon_ref = matches!(next, b'b' | b'B' | b'f' | b'F')
                && (after >= line.len() || !is_anon_ident_cont(line[after]));
            if is_anon_ref {
                out.push(b);
                i += 1;
                continue;
            }
            i += 1;
            let start = i;
            while i < line.len() && (line[i].is_ascii_alphanumeric() || line[i] == b'_') {
                i += 1;
            }
            let name = line[start..i].to_vec();
            let lno = if let Some(&existing) = name_map.get(&name) {
                existing
            } else {
                let new_lno = *local_count;
                *local_count += 1;
                name_map.insert(name, new_lno);
                new_lno
            };
            out.push(0xFE);
            out.push((lno >> 8) as u8);
            out.push((lno & 0xFF) as u8);
            continue;
        }
        if b == b'\'' || b == b'"' {
            let quote = b;
            out.push(b);
            i += 1;
            while i < line.len() && line[i] != quote {
                if line[i] == b'&' {
                    i += 1;
                    if i < line.len() && line[i] == b'&' {
                        out.push(b'&'); i += 1; continue;
                    }
                    let start = i;
                    while i < line.len() && (line[i].is_ascii_alphanumeric() || line[i] == b'_') { i += 1; }
                    let name = &line[start..i];
                    if let Some(idx) = params.iter().position(|p| {
                        p.len() == name.len() && p.iter().zip(name).all(|(a,b2)| a.eq_ignore_ascii_case(b2))
                    }) {
                        out.push(0xFF);
                        out.push((idx >> 8) as u8);
                        out.push((idx & 0xFF) as u8);
                    } else {
                        out.push(b'&');
                        out.extend_from_slice(name);
                    }
                } else {
                    out.push(line[i]);
                    i += 1;
                }
            }
            if i < line.len() { out.push(line[i]); i += 1; }
            continue;
        }
        if b.is_ascii_alphabetic() || b == b'_' {
            let prev = out.last().copied();
            let start = i;
            while i < line.len() && (line[i].is_ascii_alphanumeric() || line[i] == b'_') {
                i += 1;
            }
            let name = &line[start..i];
            if prev != Some(b'.') && prev != Some(b'\\') {
                if let Some(idx) = params.iter().position(|p| {
                    p.len() == name.len() && p.iter().zip(name.iter()).all(|(a, b2)| {
                        a.eq_ignore_ascii_case(b2)
                    })
                }) {
                    out.push(0xFF);
                    out.push((idx >> 8) as u8);
                    out.push((idx & 0xFF) as u8);
                    continue;
                }
            }
            out.extend_from_slice(name);
            continue;
        }
        out.push(b);
        i += 1;
    }
    out
}

#[inline]
fn is_anon_ident_cont(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'$' || b == b'?'
}

pub(super) fn expand_macro_body(
    template: &[u8],
    params:   &[Vec<u8>],
    args:     &[Vec<u8>],
    local_base: u32,
    records:  &mut Vec<TempRecord>,
    p1:       &mut P1Ctx<'_>,
    source:   &mut SourceStack,
) {
    let mut start = 0;
    while start <= template.len() {
        let end = template[start..].iter().position(|&b| b == b'\n')
            .map(|n| start + n)
            .unwrap_or(template.len());
        if end == start && start == template.len() { break; }

        let tline = &template[start..end];
        let next_start = if end < template.len() { end + 1 } else { template.len() };

        let expanded = expand_line(tline, params, args, local_base, p1.sym);

        let mnem = extract_mnemonic(&expanded);
        let handler_opt = p1.sym.lookup_cmd(&mnem, p1.cpu_type())
            .and_then(|s| if let Symbol::Opcode { handler, .. } = s { Some(*handler) } else { None });

        match handler_opt {
            Some(InsnHandler::Rept) => {
                let remaining = &template[next_start..];
                let (body, _, consumed) = collect_body_from_slice(remaining, p1.sym, p1.ctx);
                start = next_start + consumed;

                if !p1.is_skip {
                    let line = &expanded;
                    let mut pos = 0usize;
                    skip_spaces(line, &mut pos);
                    while pos < line.len() && !line[pos].is_ascii_whitespace() { pos += 1; }
                    skip_spaces(line, &mut pos);
                    let count = if let Ok(rpn) = parse_expr(line, &mut pos) {
                        p1.eval_const(&rpn).map(|v| v.value as u32).unwrap_or(0)
                    } else { 0 };
                    for _ in 0..count {
                        let lb = p1.next_local_base();
                        expand_macro_body(&body, &[], &[], lb, records, p1, source);
                    }
                }
                continue;
            }
            Some(InsnHandler::Irp) => {
                let remaining = &template[next_start..];
                let line = &expanded;
                let mut pos = 0usize;
                while pos < line.len() && !line[pos].is_ascii_whitespace() { pos += 1; }
                skip_spaces(line, &mut pos);
                let param_name = read_ident(line, &mut pos);
                skip_spaces(line, &mut pos);
                if pos < line.len() && line[pos] == b',' { pos += 1; }
                let irp_args = parse_macro_args(line, &mut pos);
                let irp_params = if param_name.is_empty() { vec![] } else { vec![param_name] };
                let (body, _, consumed) = collect_body_from_slice_with_params(
                    remaining, p1.sym, p1.ctx, &irp_params
                );
                start = next_start + consumed;

                if !p1.is_skip {
                    for irp_arg in &irp_args {
                        let lb = p1.next_local_base();
                        expand_macro_body(&body, &irp_params,
                            std::slice::from_ref(irp_arg), lb, records, p1, source);
                    }
                }
                continue;
            }
            Some(InsnHandler::Irpc) => {
                let remaining = &template[next_start..];
                let line = &expanded;
                let mut pos = 0usize;
                while pos < line.len() && !line[pos].is_ascii_whitespace() { pos += 1; }
                skip_spaces(line, &mut pos);
                let param_name = read_ident(line, &mut pos);
                skip_spaces(line, &mut pos);
                if pos < line.len() && line[pos] == b',' { pos += 1; }
                skip_spaces(line, &mut pos);
                let s = parse_string_or_ident(line, &mut pos);
                let irpc_params = if param_name.is_empty() { vec![] } else { vec![param_name] };
                let (body, _, consumed) = collect_body_from_slice_with_params(
                    remaining, p1.sym, p1.ctx, &irpc_params
                );
                start = next_start + consumed;

                if !p1.is_skip {
                    for &ch in &s {
                        let arg = vec![ch];
                        let lb = p1.next_local_base();
                        expand_macro_body(&body, &irpc_params,
                            std::slice::from_ref(&arg), lb, records, p1, source);
                    }
                }
                continue;
            }
            _ => {
                if should_emit_line_info(&expanded, p1, true) {
                    let line_num = source.current().line;
                    records.push(TempRecord::LineInfo {
                        line_num,
                        text: expanded.clone(),
                        is_macro: true,
                    });
                }
                parse_line(&expanded, records, p1, source);
            }
        }

        start = next_start;
    }
}

fn collect_body_from_slice(
    slice: &[u8],
    sym: &SymbolTable,
    ctx: &AssemblyContext,
) -> (Vec<u8>, u16, usize) {
    collect_body_from_slice_impl(slice, sym, ctx, &[], true)
}

fn collect_body_from_slice_with_params(
    slice: &[u8],
    sym: &SymbolTable,
    ctx: &AssemblyContext,
    params: &[Vec<u8>],
) -> (Vec<u8>, u16, usize) {
    collect_body_from_slice_impl(slice, sym, ctx, params, true)
}

fn collect_body_from_slice_impl(
    slice: &[u8],
    sym: &SymbolTable,
    ctx: &AssemblyContext,
    params: &[Vec<u8>],
    do_param_convert: bool,
) -> (Vec<u8>, u16, usize) {
    let mut body = Vec::new();
    let mut local_count = 0u16;
    let mut nest_depth = 0u32;
    let mut pos = 0;
    let mut name_map: std::collections::HashMap<Vec<u8>, u16> = std::collections::HashMap::new();

    while pos < slice.len() {
        let end = slice[pos..].iter().position(|&b| b == b'\n')
            .map(|n| pos + n)
            .unwrap_or(slice.len());
        let line = &slice[pos..end];
        let next_pos = if end < slice.len() { end + 1 } else { slice.len() };

        let mnem = extract_mnemonic(line);
        let handler = sym.lookup_cmd(&mnem, ctx.cpu.features)
            .and_then(|s| if let Symbol::Opcode { handler, .. } = s { Some(*handler) } else { None });

        match handler {
            Some(InsnHandler::MacroDef | InsnHandler::Rept | InsnHandler::Irp | InsnHandler::Irpc) => {
                nest_depth += 1;
                body.extend_from_slice(line);
                body.push(b'\n');
            }
            Some(InsnHandler::EndM) => {
                if nest_depth == 0 {
                    pos = next_pos;
                    break;
                }
                nest_depth -= 1;
                body.extend_from_slice(line);
                body.push(b'\n');
            }
            _ => {
                if do_param_convert {
                    let converted = convert_line_params(line, params, &mut local_count, &mut name_map);
                    body.extend_from_slice(&converted);
                } else {
                    body.extend_from_slice(line);
                }
                body.push(b'\n');
            }
        }

        pos = next_pos;
    }

    (body, local_count, pos)
}

fn expand_line(
    tline:      &[u8],
    _params:    &[Vec<u8>],
    args:       &[Vec<u8>],
    local_base: u32,
    sym:        &SymbolTable,
) -> Vec<u8> {
    let mut out = Vec::with_capacity(tline.len() + 16);
    let mut i = 0;
    while i < tline.len() {
        let b = tline[i];
        if b == 0xFF && i + 2 < tline.len() {
            let idx = ((tline[i+1] as usize) << 8) | (tline[i+2] as usize);
            i += 3;
            if let Some(arg) = args.get(idx) {
                out.extend_from_slice(arg);
            }
        } else if b == 0xFE && i + 2 < tline.len() {
            let lno = ((tline[i+1] as u32) << 8) | (tline[i+2] as u32);
            i += 3;
            let label = format!("??{:04X}{:04X}", local_base & 0xFFFF, lno & 0xFFFF);
            out.extend_from_slice(label.as_bytes());
        } else if b == b'%' {
            let start = i + 1;
            let mut end = start;
            while end < tline.len() && (tline[end].is_ascii_alphanumeric() || tline[end] == b'_') {
                end += 1;
            }
            if end > start {
                let name = &tline[start..end];
                if let Some(Symbol::Value { value, .. }) = sym.lookup_sym(name) {
                    let s = format!("{}", value);
                    out.extend_from_slice(s.as_bytes());
                    i = end;
                    continue;
                }
            }
            out.push(b);
            i += 1;
        } else {
            out.push(b);
            i += 1;
        }
    }
    out
}

fn extract_mnemonic(line: &[u8]) -> Vec<u8> {
    let mut pos = 0;
    if !line.is_empty() && line[0] != b' ' && line[0] != b'\t' {
        while pos < line.len() && line[pos] != b' ' && line[pos] != b'\t' && line[pos] != b';' {
            pos += 1;
        }
    }
    while pos < line.len() && (line[pos] == b' ' || line[pos] == b'\t') { pos += 1; }
    if pos < line.len() && line[pos] == b'.' { pos += 1; }
    let start = pos;
    while pos < line.len() && (line[pos].is_ascii_alphanumeric() || line[pos] == b'_') { pos += 1; }
    line[start..pos].to_ascii_lowercase()
}
