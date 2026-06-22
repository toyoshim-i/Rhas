//! Pass 1: ソース行解析 → TempRecord 生成
//!
//! オリジナルの `main.s` の pass1 ルーチンに対応。
//! ソーステキストをスキャンし、シンボルを定義しながら TempRecord 列を構築する。

use super::pseudo;
use super::temp::TempRecord;
use crate::context::AssemblyContext;
use crate::error::{ErrorCode, ErrorContext, SourcePos, WarnContext};
use crate::expr::eval::EvalValue;
use crate::expr::rpn::RPNToken;
use crate::expr::{eval_rpn, parse_expr, Rpn};
use crate::source::{ReadResult, SourceStack};
use crate::symbol::types::{DefAttrib, ExtAttrib, FirstDef, InsnHandler, SizeCode};
use crate::symbol::{Symbol, SymbolTable};
use std::collections::HashMap;

mod insn;
mod macro_exp;
mod operand;
mod optimize;
mod preprocess;
mod pseudo_dispatch;

use insn::handle_real_insn;
pub(crate) use macro_exp::{
    collect_macro_body, expand_macro_body, parse_macro_args, parse_macro_params,
};
use preprocess::{preprocess_anon_labels, preprocess_numeric_local_labels};
use pseudo_dispatch::handle_pseudo;
pub(crate) use pseudo_dispatch::{
    parse_align_n, parse_align_pad, parse_filename, parse_string_or_ident, read_ident,
};

/// Pass1 の作業状態
pub struct P1Ctx<'a> {
    pub(super) sym: &'a mut SymbolTable,
    pub(super) ctx: &'a mut AssemblyContext,
    /// .if ネスト深度（最大 64 段）
    pub(super) if_nest: u16,
    /// スキップ中の .if ネスト深度（0 = スキップしていない）
    pub(super) skip_nest: u16,
    /// スキップ中（is_if_skip）
    pub(super) is_skip: bool,
    /// 各 if-nesting レベルでマッチ済みブランチがあるか（.elseif/.else の重複実行防止）
    pub(super) if_matched: [bool; 65],
    /// .end が来たか
    pub(super) is_end: bool,
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
            sym,
            ctx,
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
        if self.ctx.is_offset_mode {
            0
        } else {
            self.ctx.section as u8
        }
    }
    pub(super) fn is_offset_mode(&self) -> bool {
        self.ctx.is_offset_mode
    }

    /// Consume and increment the local base, returning the previous value.
    pub(super) fn next_local_base(&mut self) -> u32 {
        let v = self.local_base;
        self.local_base = self.local_base.wrapping_add(1);
        v
    }
    pub(super) fn cpu_type(&self) -> u16 {
        self.ctx.cpu.features
    }
    pub(super) fn location(&self) -> u32 {
        self.ctx.location()
    }

    pub(super) fn advance(&mut self, n: u32) {
        self.ctx.advance_location(n);
    }

    /// シンボル定義（ロケーションラベル）
    pub(super) fn define_label(&mut self, name: Vec<u8>, section: u8, offset: u32) {
        let sym = Symbol::Value {
            attrib: DefAttrib::Define,
            ext_attrib: ExtAttrib::None,
            section,
            org_num: 0,
            first: FirstDef::Other,
            opt_count: 0,
            value: offset as i32,
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
    if let Symbol::Value {
        value,
        section,
        attrib,
        ..
    } = sym
    {
        if *attrib >= DefAttrib::Define {
            return Some(EvalValue {
                value: *value,
                section: *section,
            });
        }
    }
    None
}

/// Pass1: ソース → TempRecord 列
pub fn pass1(
    source: &mut SourceStack,
    ctx: &mut AssemblyContext,
    sym: &mut SymbolTable,
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
                    records.push(TempRecord::LineInfo {
                        line_num,
                        text: line.clone(),
                        is_macro: false,
                    });
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
                if p1.is_end {
                    break;
                }
            }
        }
    }

    records
}

/// 行を先読みし、`.list/.nlist` の PRN 制御擬似命令かどうか判定する。
/// 戻り値: `Some(true)=.list`, `Some(false)=.nlist`, `None=その他`
fn detect_prn_list_control(line: &[u8], p1: &P1Ctx<'_>) -> Option<bool> {
    if line.is_empty() {
        return None;
    }
    if line.first() == Some(&b'*') || line.first() == Some(&b';') {
        return None;
    }

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

    let handler = p1.sym.lookup_cmd(&mnem, p1.cpu_type()).and_then(|s| {
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
pub(super) fn should_emit_line_info(line: &[u8], p1: &P1Ctx<'_>, is_macro: bool) -> bool {
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

pub(super) fn parse_line(
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
    if line.first() == Some(&b'*') {
        return;
    }

    // 行頭の ';' → コメント行
    if line.first() == Some(&b';') {
        return;
    }

    // 空行
    if line.is_empty() {
        return;
    }

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
                    name: name.clone(),
                    section: sec,
                    offset: off,
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
    if mnem.is_empty() {
        return;
    }

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
        let h = p1.sym.lookup_cmd(&mnem, p1.cpu_type()).and_then(|s| {
            if let Symbol::Opcode { handler, .. } = s {
                Some(*handler)
            } else {
                None
            }
        });
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
        Some(Symbol::Opcode {
            handler,
            opcode,
            arch,
            ..
        }) => {
            if arch.is_pseudo() {
                Dispatch::Pseudo(*handler)
            } else {
                Dispatch::RealInsn(*handler, *opcode)
            }
        }
        Some(Symbol::Macro { .. }) => Dispatch::MacroCall,
        _ => Dispatch::Unknown,
    };

    let is_equ = matches!(
        dispatch,
        Dispatch::Pseudo(InsnHandler::Equ | InsnHandler::Set | InsnHandler::Reg)
    );

    // ロケーションラベルを先に登録（.equ/.set 以外）
    // XDef TempRecord は命令処理後に追加する（HAS互換の順序: XREF → XDEF）
    if !is_equ {
        if let Some(ref name) = label {
            let sec = p1.section_id();
            let off = p1.location();
            p1.define_label(name.clone(), sec, off);
            records.push(TempRecord::LabelDef {
                name: name.clone(),
                section: sec,
                offset: off,
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
                handler, &mnem, size, line, &mut pos, &label, records, p1, source,
            );
        }
        // ---- 実命令 ----
        Dispatch::RealInsn(handler, opcode) => {
            handle_real_insn(handler, opcode, size, line, pos, records, p1);
        }
        // ---- マクロ呼び出し ----
        Dispatch::MacroCall => {
            if let Some(Symbol::Macro {
                params,
                local_count: _,
                template,
            }) = p1.sym.lookup_cmd(&mnem, p1.cpu_type()).cloned()
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
                attrib: DefAttrib::Define,
                ext_attrib: ExtAttrib::None,
                section: v.section,
                org_num: 0,
                first: FirstDef::Other,
                opt_count: 0,
                value: v.value,
            };
            p1.sym.define(name.to_vec(), sym);
        }
    }
}

/// (name, is_global) を返す
fn parse_label(line: &[u8], pos: &mut usize) -> Option<(Vec<u8>, bool)> {
    let start = *pos;
    // '.' で始まる場合は疑似命令 → ラベルではない
    if line.get(start) == Some(&b'.') {
        return None;
    }
    let mut end = start;
    while end < line.len() {
        let b = line[end];
        if b == b':' || b == b' ' || b == b'\t' || b == b';' {
            break;
        }
        end += 1;
    }
    if end == start {
        return None;
    }
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
    if has_dot {
        *pos += 1;
    }

    // ニーモニック本体
    let start = *pos;
    while *pos < line.len() && is_mnem_char(line[*pos]) {
        *pos += 1;
    }
    let mnem_raw = &line[start..*pos];
    if mnem_raw.is_empty() {
        return (Vec::new(), None);
    }

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

pub(crate) fn skip_spaces(line: &[u8], pos: &mut usize) {
    while *pos < line.len() && matches!(line[*pos], b' ' | b'\t') {
        *pos += 1;
    }
}
