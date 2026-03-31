//! Pass 1: ソース行解析 → TempRecord 生成
//!
//! オリジナルの `main.s` の pass1 ルーチンに対応。
//! ソーステキストをスキャンし、シンボルを定義しながら TempRecord 列を構築する。

use crate::addressing::{parse_ea, parse_reg_list_mask, EffectiveAddress};
use crate::context::AssemblyContext;
use crate::error::{ErrorCode, SourcePos, warn, ErrorContext, WarnContext};
use crate::expr::{eval_rpn, parse_expr, Rpn};
use crate::expr::eval::EvalValue;
use crate::expr::rpn::RPNToken;
use crate::instructions::{encode_insn, InsnError};
use crate::options::cpu as cpuconst;
use crate::source::{ReadResult, SourceStack};
use crate::symbol::{Symbol, SymbolTable};
use crate::symbol::types::{reg, DefAttrib, ExtAttrib, FirstDef, InsnHandler, SizeCode};
use std::collections::HashMap;
use super::pseudo;
use super::temp::TempRecord;

// ----------------------------------------------------------------
// エラー型
// ----------------------------------------------------------------

// ----------------------------------------------------------------
// Pass1 コンテキスト
// ----------------------------------------------------------------

/// Pass1 の作業状態
pub struct P1Ctx<'a> {
    pub(crate) sym:      &'a mut SymbolTable,
    pub(crate) ctx:      &'a mut AssemblyContext,
    /// .if ネスト深度（最大 64 段）
    pub(crate) if_nest:  u16,
    /// スキップ中の .if ネスト深度（0 = スキップしていない）
    pub(crate) skip_nest: u16,
    /// スキップ中（is_if_skip）
    pub(crate) is_skip:  bool,
    /// 各 if-nesting レベルでマッチ済みブランチがあるか（.elseif/.else の重複実行防止）
    pub(crate) if_matched: [bool; 65],
    /// .end が来たか
    is_end:   bool,
    /// ローカルラベルベース（マクロ展開番号用、将来実装）
    local_base: u32,
    /// 匿名ローカルラベルカウンタ（@@: の通し番号）
    local_anon_count: u32,
    /// 数値ローカルラベル（`1:` / `1f` / `1b`）の定義カウンタ
    num_local_counts: HashMap<u32, u32>,
    /// 現在処理中のソース位置（エラーメッセージ用）
    current_pos: SourcePos,
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
    pub(crate) fn error_code(&mut self, code: ErrorCode, sym: Option<&[u8]>) {
        let err_ctx = match sym {
            Some(s) => ErrorContext::with_symbol(self.current_pos.clone(), code, s),
            None => ErrorContext::new(self.current_pos.clone(), code, None),
        };
        let mut stderr = std::io::stderr();
        crate::error::print_error_context(&mut stderr, &err_ctx);
        self.ctx.add_error();
    }

    pub(crate) fn warn_code(&mut self, code: crate::error::WarnCode, sym: Option<&[u8]>) {
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

    pub(crate) fn section_id(&self) -> u8 {
        if self.ctx.is_offset_mode { 0 } else { self.ctx.section as u8 }
    }
    pub(crate) fn is_offset_mode(&self) -> bool { self.ctx.is_offset_mode }

    /// Consume and increment the local base, returning the previous value.
    pub(crate) fn next_local_base(&mut self) -> u32 {
        let v = self.local_base;
        self.local_base = self.local_base.wrapping_add(1);
        v
    }
    pub(crate) fn cpu_type(&self)   -> u16 { self.ctx.cpu_type }
    pub(crate) fn location(&self)   -> u32 { self.ctx.location() }

    pub(crate) fn advance(&mut self, n: u32) {
        self.ctx.advance_location(n);
    }

    /// シンボル定義（ロケーションラベル）
    fn define_label(&mut self, name: Vec<u8>, section: u8, offset: u32) {
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
    pub(crate) fn eval_const(&self, rpn: &Rpn) -> Option<EvalValue> {
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

// ----------------------------------------------------------------
// Pass1 メイン
// ----------------------------------------------------------------

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

// ----------------------------------------------------------------
// 行解析
// ----------------------------------------------------------------

/// 匿名ローカルラベル（@@: / @b / @f）を行内で展開する。
/// @@: → @@{count}: に展開（is_anon_def=true を返す）
/// @b → @@{count-1}、@f → @@{count} に展開する。
/// コメント（;）以降は処理しない。
fn preprocess_anon_labels(line: &[u8], count: u32) -> (Vec<u8>, bool) {
    let mut result = Vec::with_capacity(line.len() + 8);
    let mut i = 0;
    let mut is_anon_def = false;

    // 行頭 @@: / @@:: の検出
    if line.starts_with(b"@@") && line.get(2) == Some(&b':') {
        is_anon_def = true;
        let label = format!("@@{}", count);
        result.extend_from_slice(label.as_bytes());
        // ':' から後ろはそのまま
        i = 2; // ':' の位置から再開
    }

    // 残りの行を処理（@b / @f 置換）
    while i < line.len() {
        let b = line[i];
        // コメント → そのまま残す
        if b == b';' {
            result.extend_from_slice(&line[i..]);
            break;
        }
        // @b / @f の検出（@@ や @name とは区別する）
        if b == b'@' && i + 1 < line.len() {
            let next = line[i + 1];
            let after = i + 2;
            let is_end = after >= line.len() || !is_anon_ident_cont(line[after]);
            if next == b'b' && is_end {
                // @b → 最後に定義した @@: ラベルの名前
                let name = if count > 0 {
                    format!("@@{}", count - 1)
                } else {
                    "@@_invalid_@b".to_string()
                };
                result.extend_from_slice(name.as_bytes());
                i += 2;
                continue;
            }
            if next == b'f' && is_end {
                // @f → 次に定義される @@: ラベルの名前
                let name = format!("@@{}", count);
                result.extend_from_slice(name.as_bytes());
                i += 2;
                continue;
            }
        }
        result.push(b);
        i += 1;
    }
    (result, is_anon_def)
}

/// 匿名ラベル置換後の識別子継続文字かどうか
#[inline]
fn is_anon_ident_cont(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'$' || b == b'?'
}

/// 数値ローカルラベル（`1:` / `1f` / `1b`）を一意名へ展開する。
///
/// 例:
/// - `1:`  -> `__n1__0:`
/// - `1f`  -> `__n1__0` （次の `1:`）
/// - `1b`  -> `__n1__0` （直前の `1:`）
fn preprocess_numeric_local_labels(line: &[u8], counts: &mut HashMap<u32, u32>) -> Vec<u8> {
    let mut result = Vec::with_capacity(line.len() + 16);
    let mut i = 0usize;
    let mut def_num: Option<u32> = None;
    let mut in_single = false;
    let mut in_double = false;

    // 行頭の `N:` 定義を先に処理
    let mut j = 0usize;
    while j < line.len() && line[j].is_ascii_digit() {
        j += 1;
    }
    if j > 0 && line.get(j) == Some(&b':') {
        if let Ok(num_str) = std::str::from_utf8(&line[..j]) {
            if let Ok(num) = num_str.parse::<u32>() {
                let idx = *counts.get(&num).unwrap_or(&0);
                let label = format!("__n{}__{}", num, idx);
                result.extend_from_slice(label.as_bytes());
                i = j; // ':' から後ろを通常処理
                def_num = Some(num);
            }
        }
    }

    while i < line.len() {
        let b = line[i];
        if b == b';' {
            result.extend_from_slice(&line[i..]);
            break;
        }
        if !in_double && b == b'\'' {
            in_single = !in_single;
            result.push(b);
            i += 1;
            continue;
        }
        if !in_single && b == b'"' {
            in_double = !in_double;
            result.push(b);
            i += 1;
            continue;
        }
        if in_single || in_double {
            result.push(b);
            i += 1;
            continue;
        }

        if b.is_ascii_digit() {
            let prev = if i > 0 { Some(line[i - 1]) } else { None };
            // $2b / %1010 / 0x2f のような数値リテラルは置換しない。
            let numeric_prefix = matches!(prev, Some(b'$' | b'%'))
                || (i >= 2 && (line[i - 2] == b'0') && matches!(line[i - 1], b'x' | b'X'));
            let left_boundary = (i == 0 || !is_num_local_ident_cont(line[i - 1])) && !numeric_prefix;
            if left_boundary {
                let mut k = i;
                while k < line.len() && line[k].is_ascii_digit() {
                    k += 1;
                }
                let suffix = line.get(k).copied();
                if let Some(suffix_char @ (b'f' | b'b')) = suffix {
                    let after = k + 1;
                    let right_boundary = after >= line.len() || !is_num_local_ident_cont(line[after]);
                    if right_boundary {
                        if let Ok(num_str) = std::str::from_utf8(&line[i..k]) {
                            if let Ok(num) = num_str.parse::<u32>() {
                                let cnt = *counts.get(&num).unwrap_or(&0);
                                let ref_idx = match suffix_char {
                                    b'b' => cnt.saturating_sub(1),
                                    _ => cnt,
                                };
                                let name = if suffix_char == b'b' && cnt == 0 {
                                    format!("__n{}_invalid_b", num)
                                } else {
                                    format!("__n{}__{}", num, ref_idx)
                                };
                                result.extend_from_slice(name.as_bytes());
                                i = after;
                                continue;
                            }
                        }
                    }
                }
            }
        }

        result.push(b);
        i += 1;
    }

    if let Some(num) = def_num {
        let e = counts.entry(num).or_insert(0);
        *e += 1;
    }
    result
}

#[inline]
fn is_num_local_ident_cont(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'$' || b == b'?'
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

// ----------------------------------------------------------------
// ':=' 代入処理（SET の糖衣構文）
// ----------------------------------------------------------------

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

// ----------------------------------------------------------------
// ラベル解析
// ----------------------------------------------------------------

/// ラベルを解析して返す（pos を進める）
/// (name, is_global) を返す
/// `LABEL:`  → (name, false)
/// `LABEL::` → (name, true)  ← グローバルラベル（自動 .xdef 相当）
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

// ----------------------------------------------------------------
// ニーモニック + サイズ解析
// ----------------------------------------------------------------

/// ニーモニックとサイズを解析する
/// `.dc.b` → (b"dc", Some(Byte))
/// `move.w` → (b"move", Some(Word))
/// `nop` → (b"nop", None)
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

    let mnem = to_lowercase(mnem_raw);
    (mnem, size)
}

fn is_mnem_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn to_lowercase(s: &[u8]) -> Vec<u8> {
    s.iter().map(|c| c.to_ascii_lowercase()).collect()
}

pub(crate) fn skip_spaces(line: &[u8], pos: &mut usize) {
    while *pos < line.len() && matches!(line[*pos], b' ' | b'\t') {
        *pos += 1;
    }
}

// ----------------------------------------------------------------
// 実命令処理
// ----------------------------------------------------------------

fn handle_real_insn(
    handler:  InsnHandler,
    opcode:   u16,
    size:     Option<SizeCode>,
    line:     &[u8],
    pos:      usize,
    records:  &mut Vec<TempRecord>,
    p1:       &mut P1Ctx<'_>,
) {
    // HAS のデフォルトサイズはワード（サフィックスなし → .w 相当）
    let sz = size.unwrap_or(SizeCode::Word);
    let cpu = p1.cpu_type();

    // 分岐命令（ターゲットを RPN として保持）
    if matches!(handler, InsnHandler::Bcc | InsnHandler::JBcc) {
        let target = parse_branch_target(line, pos);
        if let Some(rpn) = target {
            let byte_sz = super::temp::branch_word_size(size);
            p1.advance(byte_sz);
            records.push(TempRecord::Branch {
                opcode,
                target: rpn,
                req_size: size,
                cur_size: size,
                suppressed: false,
            });
        } else {
            // オペランドなし (NOP/RTS 等)
            if let Ok(bytes) = encode_insn(opcode, handler, sz, &[]) {
                p1.advance(bytes.len() as u32);
                records.push(TempRecord::Const(bytes));
            }
        }
        return;
    }

    // FBcc: ターゲットを RPN として保持（Pass3 でPC相対計算）
    if matches!(handler, InsnHandler::FBcc) {
        if let Some(rpn) = parse_branch_target(line, pos) {
            let opcode = (opcode & !0x0E00) | ((u16::from(p1.ctx.fpid & 0x07)) << 9);
            let req = size.unwrap_or(SizeCode::None);
            let byte_size = match req {
                SizeCode::Long => 6,
                _ => 4, // .w または自動
            };
            p1.advance(byte_size);
            records.push(TempRecord::DeferredInsn {
                base: opcode,
                handler,
                size: req,
                ops: vec![EffectiveAddress::AbsLong(rpn)],
                byte_size,
            });
        }
        return;
    }

    // DBcc: ターゲットを RPN として保持
    if matches!(handler, InsnHandler::DBcc) {
        let mut ops = parse_operands(line, pos, p1.sym, cpu);
        if ops.len() == 2 {
            // ops[0] = Dn, ops[1] = label (as AbsLong RPN)
            let dn = ops.remove(0);
            let target = if let EffectiveAddress::AbsLong(rpn) = ops.remove(0) {
                rpn
            } else {
                vec![RPNToken::Value(0), RPNToken::End]
            };
            // estimate 4 bytes: opcode(2) + dn + offset(2)
            // Actually DBcc is 2(opcode) + 2(offset) = 4 bytes, Dn is encoded in opcode
            p1.advance(4);
            records.push(TempRecord::DeferredInsn {
                base: opcode, handler, size: sz,
                ops: vec![dn, EffectiveAddress::AbsLong(target)],
                byte_size: 4,
            });
        }
        return;
    }

    // FDBcc: Dn,ターゲットを保持（Pass3 でPC相対計算）
    if matches!(handler, InsnHandler::FDBcc) {
        let mut ops = parse_operands(line, pos, p1.sym, cpu);
        if ops.len() == 2 {
            let opcode = (opcode & !0x0E00) | ((u16::from(p1.ctx.fpid & 0x07)) << 9);
            let dn = ops.remove(0);
            let target = if let EffectiveAddress::AbsLong(rpn) = ops.remove(0) {
                rpn
            } else {
                vec![RPNToken::Value(0), RPNToken::End]
            };
            p1.advance(6);
            records.push(TempRecord::DeferredInsn {
                base: opcode,
                handler,
                size: SizeCode::None,
                ops: vec![dn, EffectiveAddress::AbsLong(target)],
                byte_size: 6,
            });
        }
        return;
    }

    // 通常命令
    let mut ops = parse_operands(line, pos, &*p1.sym, cpu);
    let mut enc_size = sz;

    // JMP/JSR 最適化（安全に判定できるケースのみ）
    if matches!(handler, InsnHandler::JmpJsr) && p1.ctx.opts.opt_jmp_jsr && ops.len() == 1 {
        match &ops[0] {
            // jmp/jsr (2,pc): jmpは削除、jsrはpea (2,pc)
            EffectiveAddress::PcDisp(disp) if disp.size.is_none() && disp.const_val == Some(2) => {
                if opcode == 0x4EC0 {
                    // jmp (2,pc) は命令自体を削除
                    return;
                }
                if opcode == 0x4E80 {
                    // jsr (2,pc) → pea (2,pc)
                    let bytes = vec![0x48, 0x7A, 0x00, 0x02];
                    p1.advance(bytes.len() as u32);
                    records.push(TempRecord::Const(bytes));
                    return;
                }
            }
            // jmp/jsr label（サイズ指定なし）→ jbra/jbsr 相当の分岐最適化パスへ渡す
            // オリジナルは定数ターゲット（jmp $FF0038 など）を除いて変換する。
            EffectiveAddress::AbsLong(rpn) if !single_operand_has_explicit_long_suffix(line, pos) => {
                let is_const_abs = matches!(p1.eval_const(rpn), Some(v) if v.section == 0);
                if !is_const_abs {
                    let bcc_opcode = if opcode == 0x4E80 { 0x6100 } else { 0x6000 };
                    let byte_sz = super::temp::branch_word_size(None);
                    p1.advance(byte_sz);
                    records.push(TempRecord::Branch {
                        opcode: bcc_opcode,
                        target: rpn.clone(),
                        req_size: None,
                        cur_size: None,
                        suppressed: false,
                    });
                    return;
                }
            }
            // jmp/jsr (label,pc)（サイズ指定なし・非定数）も分岐最適化へ渡す
            EffectiveAddress::PcDisp(disp)
                if disp.size.is_none() && disp.const_val.is_none() =>
            {
                let bcc_opcode = if opcode == 0x4E80 { 0x6100 } else { 0x6000 };
                let byte_sz = super::temp::branch_word_size(None);
                p1.advance(byte_sz);
                records.push(TempRecord::Branch {
                    opcode: bcc_opcode,
                    target: disp.rpn.clone(),
                    req_size: None,
                    cur_size: None,
                    suppressed: false,
                });
                return;
            }
            _ => {}
        }
    }

    // 命令最適化（-c4）
    let mut handler = handler;
    let mut opcode = opcode;
    if matches!(
        handler,
        InsnHandler::FMove
            | InsnHandler::FMoveM
            | InsnHandler::FMoveCr
            | InsnHandler::FSinCos
            | InsnHandler::FArith
            | InsnHandler::FCmp
            | InsnHandler::FTst
            | InsnHandler::FNop
            | InsnHandler::FSave
            | InsnHandler::FRestore
            | InsnHandler::FBcc
            | InsnHandler::FDBcc
    ) {
        opcode = (opcode & !0x0E00) | ((u16::from(p1.ctx.fpid & 0x07)) << 9);
    }

    // MOVE.l #imm,Dn → MOVEQ #imm,Dn（#-128..255）
    // MOVE.b/.w #0,Dn → CLR.b/.w Dn
    if matches!(handler, InsnHandler::Move) && ops.len() >= 2 {
        if let (EffectiveAddress::Immediate(rpn), EffectiveAddress::DataReg(_)) = (&ops[0], &ops[1]) {
            if let Some(ev) = p1.eval_const(rpn) {
                if ev.section == 0 {
                    if enc_size == SizeCode::Long
                        && !p1.ctx.opts.no_quick
                        && ev.value >= -128
                        && ev.value <= 127
                    {
                        handler = InsnHandler::MoveQ;
                        opcode = 0x7000;
                    } else if p1.ctx.opts.opt_move0
                        && ev.value == 0
                        && matches!(enc_size, SizeCode::Byte | SizeCode::Word)
                    {
                        handler = InsnHandler::Clr;
                        opcode = 0x4200;
                        ops = vec![ops[1].clone()];
                    }
                }
            }
        }
    }

    // CLR.l Dn → MOVEQ #0,Dn（68000/68010のみ）
    if matches!(handler, InsnHandler::Clr)
        && p1.ctx.opts.opt_clr
        && enc_size == SizeCode::Long
        && p1.ctx.cpu_number < 68020
        && ops.len() == 1
        && matches!(ops[0], EffectiveAddress::DataReg(_))
    {
        handler = InsnHandler::MoveQ;
        opcode = 0x7000;
        ops = vec![
            EffectiveAddress::Immediate(vec![RPNToken::Value(0), RPNToken::End]),
            ops[0].clone(),
        ];
    }

    // CMP #0,Dn → TST Dn
    if matches!(handler, InsnHandler::Cmp)
        && p1.ctx.opts.opt_cmp0
        && ops.len() == 2
        && matches!(ops[1], EffectiveAddress::DataReg(_))
    {
        if let EffectiveAddress::Immediate(rpn) = &ops[0] {
            if let Some(ev) = p1.eval_const(rpn) {
                if ev.section == 0 && ev.value == 0 {
                    handler = InsnHandler::Tst;
                    opcode = 0x4A00;
                    ops = vec![ops[1].clone()];
                }
            }
        }
    }

    // CMPI #0,<ea> → TST <ea>
    if matches!(handler, InsnHandler::CmpI)
        && p1.ctx.opts.opt_cmpi0
        && ops.len() == 2
    {
        if let EffectiveAddress::Immediate(rpn) = &ops[0] {
            if let Some(ev) = p1.eval_const(rpn) {
                if ev.section == 0 && ev.value == 0 {
                    handler = InsnHandler::Tst;
                    opcode = 0x4A00;
                    ops = vec![ops[1].clone()];
                }
            }
        }
    }

    // SUBI/ADDI #imm(1-8),<ea> → SUBQ/ADDQ
    if matches!(handler, InsnHandler::SubAddI)
        && !p1.ctx.opts.no_quick
        && ops.len() >= 2
    {
        if let EffectiveAddress::Immediate(rpn) = &ops[0] {
            if let Some(ev) = p1.eval_const(rpn) {
                if ev.section == 0 && ev.value >= 1 && ev.value <= 8 {
                    handler = InsnHandler::SubAddQ;
                    opcode = if (opcode & 0x0200) != 0 { 0x5000 } else { 0x5100 };
                }
            }
        }
    }

    // ADD/SUB #imm(1-8), <ea> → ADDQ/SUBQ
    if matches!(handler, InsnHandler::SubAdd)
        && !p1.ctx.opts.no_quick
        && ops.len() >= 2
    {
        if let EffectiveAddress::Immediate(rpn) = &ops[0] {
            if let Some(ev) = p1.eval_const(rpn) {
                if ev.section == 0 && ev.value >= 1 && ev.value <= 8 {
                    handler = InsnHandler::SubAddQ;
                    opcode = if opcode & 0x4000 != 0 { 0x5000 } else { 0x5100 };
                }
            }
        }
    }

    // MOVEA.L #d16,An → MOVEA.W #d16,An
    if matches!(handler, InsnHandler::MoveA)
        && p1.ctx.opts.opt_movea
        && enc_size == SizeCode::Long
        && ops.len() == 2
        && matches!(ops[1], EffectiveAddress::AddrReg(_))
    {
        if let EffectiveAddress::Immediate(rpn) = &ops[0] {
            if let Some(ev) = p1.eval_const(rpn) {
                if ev.section == 0 && ev.value >= -32768 && ev.value <= 32767 {
                    enc_size = SizeCode::Word;
                }
            }
        }
    }

    // CMPA #0,An → TST.L An（68020+）
    if matches!(handler, InsnHandler::CmpA)
        && p1.ctx.opts.opt_cmpa
        && enc_size == SizeCode::Long
        && p1.ctx.cpu_number >= 68020
        && ops.len() == 2
        && matches!(ops[1], EffectiveAddress::AddrReg(_))
    {
        if let EffectiveAddress::Immediate(rpn) = &ops[0] {
            if let Some(ev) = p1.eval_const(rpn) {
                if ev.section == 0 && ev.value == 0 {
                    handler = InsnHandler::Tst;
                    opcode = 0x4A00;
                    ops = vec![ops[1].clone()];
                }
            }
        }
    }

    // CMPA.L #d16,An → CMPA.W #d16,An
    if matches!(handler, InsnHandler::CmpA)
        && p1.ctx.opts.opt_cmpa
        && enc_size == SizeCode::Long
        && ops.len() == 2
        && matches!(ops[1], EffectiveAddress::AddrReg(_))
    {
        if let EffectiveAddress::Immediate(rpn) = &ops[0] {
            if let Some(ev) = p1.eval_const(rpn) {
                if ev.section == 0 && ev.value >= -32768 && ev.value <= 32767 {
                    enc_size = SizeCode::Word;
                }
            }
        }
    }

    // LEA 最適化:
    //   LEA (An),An / LEA (0,An),An → 削除
    //   LEA (d,An),An (d=-8..-1,1..8) → SUBQ/ADDQ.W #|d|,An
    if matches!(handler, InsnHandler::Lea)
        && p1.ctx.opts.opt_lea
        && ops.len() == 2
    {
        if let (src, EffectiveAddress::AddrReg(dst_an)) = (&ops[0], &ops[1]) {
            match src {
                EffectiveAddress::AddrRegInd(src_an) if src_an == dst_an => {
                    return;
                }
                EffectiveAddress::AddrRegDisp { an: src_an, disp }
                    if src_an == dst_an =>
                {
                    let disp_const = disp.const_val.or_else(|| {
                        p1.eval_const(&disp.rpn)
                            .and_then(|ev| if ev.section == 0 { Some(ev.value) } else { None })
                    });
                    if let Some(d) = disp_const {
                        if d == 0 {
                            return;
                        }
                        if (1..=8).contains(&d) || (-8..=-1).contains(&d) {
                            handler = InsnHandler::SubAddQ;
                            opcode = if d > 0 { 0x5000 } else { 0x5100 };
                            enc_size = SizeCode::Word;
                            let imm = if d > 0 { d } else { -d };
                            ops = vec![
                                EffectiveAddress::Immediate(vec![RPNToken::Value(imm as u32), RPNToken::End]),
                                EffectiveAddress::AddrReg(*dst_an),
                            ];
                        }
                    }
                }
                _ => {}
            }
        }
    }

    // ASL #1,Dn → ADD Dn,Dn（68060以外）
    if matches!(handler, InsnHandler::Asl)
        && p1.ctx.opts.opt_asl
        && p1.ctx.cpu_number < 68060
        && ops.len() == 2
    {
        if let (EffectiveAddress::Immediate(rpn), EffectiveAddress::DataReg(dn)) = (&ops[0], &ops[1]) {
            if let Some(ev) = p1.eval_const(rpn) {
                if ev.section == 0 && ev.value == 1 {
                    handler = InsnHandler::SubAdd;
                    opcode = 0xD000; // ADD
                    ops = vec![EffectiveAddress::DataReg(*dn), EffectiveAddress::DataReg(*dn)];
                }
            }
        }
    }

    // no_null_disp: displacement=0 の (An) 形式への最適化を抑制
    if p1.ctx.opts.no_null_disp {
        for ea in &mut ops {
            if let EffectiveAddress::AddrRegDisp { disp, .. } = ea {
                if disp.size.is_none() && disp.const_val == Some(0) {
                    disp.size = Some(crate::addressing::DispSize::Word);
                } else if disp.size.is_none() && disp.const_val.is_none() {
                    // const_val 未設定でも rpn が定数 0 なら size を設定
                    if let Some(ev) = p1.eval_const(&disp.rpn) {
                        if ev.section == 0 && ev.value == 0 {
                            disp.size = Some(crate::addressing::DispSize::Word);
                        }
                    }
                }
            }
        }
    }

    match encode_insn(opcode, handler, enc_size, &ops) {
        Ok(bytes) => {
            p1.advance(bytes.len() as u32);
            records.push(TempRecord::Const(bytes));
        }
        Err(InsnError::DeferToLinker) => {
            // シンボル参照あり → 現時点で定数解決できるものは確定する
            // （.set の時系列値を保持するため）。未確定は Pass3 に延期。
            let can_freeze_now = ops.iter().all(|ea| !ea_has_dynamic_ref(ea, p1.sym));
            if can_freeze_now {
                let resolved: Vec<EffectiveAddress> = ops.iter()
                    .map(|ea| resolve_ea_const_for_size(ea, p1.sym, p1.ctx.opts.no_null_disp))
                    .collect();
                match encode_insn(opcode, handler, enc_size, &resolved) {
                    Ok(bytes) => {
                        p1.advance(bytes.len() as u32);
                        records.push(TempRecord::Const(bytes));
                    }
                    Err(_) => {
                        let byte_size = estimate_insn_size(opcode, handler, enc_size, &ops);
                        p1.advance(byte_size);
                        records.push(TempRecord::DeferredInsn {
                            base: opcode, handler, size: enc_size, ops, byte_size,
                        });
                    }
                }
            } else {
                let byte_size = estimate_insn_size(opcode, handler, enc_size, &ops);
                p1.advance(byte_size);
                records.push(TempRecord::DeferredInsn {
                    base: opcode, handler, size: enc_size, ops, byte_size,
                });
            }
        }
        Err(_) => {
            p1.error_code(ErrorCode::Expr, None);
        }
    }
}

fn single_operand_has_explicit_long_suffix(line: &[u8], pos: usize) -> bool {
    let mut end = line.len();
    if let Some(i) = line[pos..].iter().position(|&b| b == b';') {
        end = pos + i;
    }
    let mut s = &line[pos..end];
    while !s.is_empty() && matches!(s[0], b' ' | b'\t') { s = &s[1..]; }
    while !s.is_empty() && matches!(s[s.len() - 1], b' ' | b'\t') { s = &s[..s.len() - 1]; }
    let sl = to_lowercase(s);
    sl.ends_with(b".l")
}

/// 分岐命令のターゲット RPN を解析する
/// オペランドがなければ None を返す（NOP/RTS 等）
fn parse_branch_target(line: &[u8], mut pos: usize) -> Option<Rpn> {
    skip_spaces(line, &mut pos);
    if pos >= line.len() || line[pos] == b';' {
        return None; // no operand
    }
    let mut p = pos;
    parse_expr(line, &mut p).ok()
}

/// operand → EffectiveAddress 列
/// Bitfield {offset:width} 内の値は AbsLong/AbsShort ではなく Immediate として扱う
fn abs_to_imm(ea: EffectiveAddress) -> EffectiveAddress {
    match ea {
        EffectiveAddress::AbsLong(rpn) | EffectiveAddress::AbsShort(rpn) => {
            EffectiveAddress::Immediate(rpn)
        }
        other => other,
    }
}

fn parse_operands(
    line:     &[u8],
    mut pos:  usize,
    sym:      &SymbolTable,
    cpu_type: u16,
) -> Vec<EffectiveAddress> {
    fn fp_mask_bit(fp_idx: u8) -> u16 {
        1u16 << (7 - (fp_idx & 7))
    }

    fn parse_fp_reg_list_token(
        line: &[u8],
        pos: &mut usize,
        sym: &SymbolTable,
        cpu_type: u16,
    ) -> Option<EffectiveAddress> {
        let saved = *pos;

        // 先頭 FPn を読む
        let start = *pos;
        let mut end = start;
        if end >= line.len() || !line[end].is_ascii_alphabetic() {
            return None;
        }
        while end < line.len() && (line[end].is_ascii_alphanumeric() || line[end] == b'_') {
            end += 1;
        }
        let regno = match sym.lookup_reg(&line[start..end], cpu_type) {
            Some(Symbol::Register { regno, .. }) => *regno,
            _ => return None,
        };
        if !(reg::FP0..=reg::FP7).contains(&regno) {
            return None;
        }

        let mut last_fp = regno - reg::FP0;
        let mut mask = fp_mask_bit(last_fp);
        *pos = end;

        // 区切り（/ または -）がなければリストではない
        let mut p = *pos;
        skip_spaces(line, &mut p);
        if p >= line.len() || (line[p] != b'/' && line[p] != b'-') {
            *pos = saved;
            return None;
        }

        loop {
            skip_spaces(line, pos);
            if *pos >= line.len() {
                break;
            }
            let sep = line[*pos];
            if sep != b'/' && sep != b'-' {
                break;
            }
            *pos += 1;
            skip_spaces(line, pos);

            let rs = *pos;
            let mut re = rs;
            if re >= line.len() || !line[re].is_ascii_alphabetic() {
                *pos = saved;
                return None;
            }
            while re < line.len() && (line[re].is_ascii_alphanumeric() || line[re] == b'_') {
                re += 1;
            }
            let regno2 = match sym.lookup_reg(&line[rs..re], cpu_type) {
                Some(Symbol::Register { regno, .. }) => *regno,
                _ => {
                    *pos = saved;
                    return None;
                }
            };
            if !(reg::FP0..=reg::FP7).contains(&regno2) {
                *pos = saved;
                return None;
            }
            *pos = re;
            let fp2 = regno2 - reg::FP0;

            if sep == b'-' {
                // 範囲指定: 直前の要素と今回要素の範囲を埋める
                let from = last_fp;
                let lo = from.min(fp2);
                let hi = from.max(fp2);
                for i in lo..=hi {
                    mask |= fp_mask_bit(i);
                }
            } else {
                mask |= fp_mask_bit(fp2);
            }
            last_fp = fp2;
        }

        Some(EffectiveAddress::Immediate(vec![RPNToken::ValueWord(mask), RPNToken::End]))
    }

    fn parse_fp_ctrl_list_token(
        line: &[u8],
        pos: &mut usize,
        sym: &SymbolTable,
        cpu_type: u16,
    ) -> Option<EffectiveAddress> {
        let saved = *pos;

        let parse_one = |s: &[u8]| -> Option<u16> {
            match sym.lookup_reg(s, cpu_type) {
                Some(Symbol::Register { regno, .. }) => match *regno {
                    reg::FPCR => Some(0x1000),
                    reg::FPSR => Some(0x0800),
                    reg::FPIAR => Some(0x0400),
                    _ => None,
                },
                _ => None,
            }
        };

        let mut p = *pos;
        let mut mask: u16 = 0;
        let mut any = false;
        loop {
            if p >= line.len() || !line[p].is_ascii_alphabetic() {
                break;
            }
            let start = p;
            while p < line.len() && (line[p].is_ascii_alphanumeric() || line[p] == b'_') {
                p += 1;
            }
            if let Some(m) = parse_one(&line[start..p]) {
                mask |= m;
                any = true;
            } else {
                break;
            }
            let mut q = p;
            skip_spaces(line, &mut q);
            if q < line.len() && line[q] == b'/' {
                p = q + 1;
                skip_spaces(line, &mut p);
                continue;
            }
            p = q;
            break;
        }

        if !any {
            *pos = saved;
            return None;
        }
        *pos = p;
        Some(EffectiveAddress::Immediate(vec![RPNToken::ValueWord(mask), RPNToken::End]))
    }

    fn parse_fp_pair_token(
        line: &[u8],
        pos: &mut usize,
        sym: &SymbolTable,
        cpu_type: u16,
    ) -> Option<EffectiveAddress> {
        let saved = *pos;

        let parse_fp = |s: &[u8]| -> Option<u8> {
            match sym.lookup_reg(s, cpu_type) {
                Some(Symbol::Register { regno, .. }) if (reg::FP0..=reg::FP7).contains(regno) => {
                    Some(*regno - reg::FP0)
                }
                _ => None,
            }
        };

        // FPc
        let start_c = *pos;
        let mut end_c = start_c;
        if end_c >= line.len() || !line[end_c].is_ascii_alphabetic() {
            return None;
        }
        while end_c < line.len() && (line[end_c].is_ascii_alphanumeric() || line[end_c] == b'_') {
            end_c += 1;
        }
        let fp_c = parse_fp(&line[start_c..end_c])?;

        // :
        let mut p = end_c;
        skip_spaces(line, &mut p);
        if p >= line.len() || line[p] != b':' {
            *pos = saved;
            return None;
        }
        p += 1;
        skip_spaces(line, &mut p);

        // FPs
        let start_s = p;
        let mut end_s = start_s;
        if end_s >= line.len() || !line[end_s].is_ascii_alphabetic() {
            *pos = saved;
            return None;
        }
        while end_s < line.len() && (line[end_s].is_ascii_alphanumeric() || line[end_s] == b'_') {
            end_s += 1;
        }
        let fp_s = match parse_fp(&line[start_s..end_s]) {
            Some(v) => v,
            None => {
                *pos = saved;
                return None;
            }
        };

        *pos = end_s;
        // 0x8000 を FPc:FPs の識別マーカーとして使用。
        let encoded = 0x8000u16 | ((fp_c as u16) << 4) | (fp_s as u16);
        Some(EffectiveAddress::Immediate(vec![RPNToken::ValueWord(encoded), RPNToken::End]))
    }

    fn parse_fp_register_token(
        line: &[u8],
        pos: &mut usize,
        sym: &SymbolTable,
        cpu_type: u16,
    ) -> Option<EffectiveAddress> {
        let start = *pos;
        if start >= line.len() {
            return None;
        }
        let c = line[start];
        if !c.is_ascii_alphabetic() && c != b'_' {
            return None;
        }
        let mut end = start + 1;
        while end < line.len() {
            let b = line[end];
            if b.is_ascii_alphanumeric() || b == b'_' {
                end += 1;
            } else {
                break;
            }
        }
        let name = &line[start..end];
        let regno = match sym.lookup_reg(name, cpu_type) {
            Some(Symbol::Register { regno, .. }) => *regno,
            _ => return None,
        };
        let ea = match regno {
            reg::FP0..=reg::FP7 => EffectiveAddress::FpReg(regno - reg::FP0),
            reg::FPCR => EffectiveAddress::FpCtrlReg(0),
            reg::FPSR => EffectiveAddress::FpCtrlReg(1),
            reg::FPIAR => EffectiveAddress::FpCtrlReg(2),
            _ => return None,
        };
        *pos = end;
        Some(ea)
    }

    let mut ops = Vec::new();
    skip_spaces(line, &mut pos);

    loop {
        if pos >= line.len() || line[pos] == b';' { break; }
        match parse_fp_ctrl_list_token(line, &mut pos, sym, cpu_type)
            .map(Ok)
            .unwrap_or_else(|| parse_fp_reg_list_token(line, &mut pos, sym, cpu_type)
            .map(Ok)
            .unwrap_or_else(|| parse_fp_pair_token(line, &mut pos, sym, cpu_type)
            .map(Ok)
            .unwrap_or_else(|| parse_fp_register_token(line, &mut pos, sym, cpu_type)
            .map(Ok)
            .unwrap_or_else(|| parse_ea(line, &mut pos, sym, cpu_type)))))
        {
            Ok(ea) => {
                ops.push(ea);
                // Bitfield suffix {offset:width}
                if pos < line.len() && line[pos] == b'{' {
                    pos += 1;
                    skip_spaces(line, &mut pos);
                    match parse_ea(line, &mut pos, sym, cpu_type) {
                        Ok(off) => ops.push(abs_to_imm(off)),
                        Err(_) => break,
                    }
                    skip_spaces(line, &mut pos);
                    if pos < line.len() && line[pos] == b':' {
                        pos += 1;
                        skip_spaces(line, &mut pos);
                        match parse_ea(line, &mut pos, sym, cpu_type) {
                            Ok(w) => ops.push(abs_to_imm(w)),
                            Err(_) => break,
                        }
                    }
                    skip_spaces(line, &mut pos);
                    if pos < line.len() && line[pos] == b'}' { pos += 1; }
                }
                // Register pair Dn:Dm or EA pair (An):(Am) for CAS2/MULS.L etc.
                if pos < line.len() && line[pos] == b':' {
                    let save = pos;
                    pos += 1;
                    skip_spaces(line, &mut pos);
                    match parse_ea(line, &mut pos, sym, cpu_type) {
                        Ok(pair) => ops.push(pair),
                        Err(_) => { pos = save; }
                    }
                }
            }
            Err(_) => break,
        }
        skip_spaces(line, &mut pos);
        if pos < line.len() && line[pos] == b',' {
            pos += 1;
            skip_spaces(line, &mut pos);
        } else {
            break;
        }
    }
    ops
}

/// 命令の推定バイト数（シンボル参照を 0 に置換してエンコード）
fn estimate_insn_size(
    base: u16, handler: InsnHandler, size: SizeCode, ops: &[EffectiveAddress]
) -> u32 {
    let placeholder: Vec<EffectiveAddress> =
        ops.iter().map(placeholder_ea).collect();
    match encode_insn(base, handler, size, &placeholder) {
        Ok(bytes) => bytes.len() as u32,
        Err(_) => {
            // フォールバック: EA 拡張ワードサイズの和
            2 + ops.iter().map(ea_ext_words).sum::<u32>()
        }
    }
}

/// EA 内の RPN を pass1 シンボルテーブルで解決して定数に置換する（サイズ推定精度向上のため）
fn resolve_ea_const_for_size(ea: &EffectiveAddress, sym: &SymbolTable, no_null_disp: bool) -> EffectiveAddress {
    use crate::addressing::Displacement;
    let lookup = |name: &[u8]| -> Option<EvalValue> {
        sym.lookup_sym(name).and_then(|s| {
            if let Symbol::Value { value, section, .. } = s {
                Some(EvalValue { value: *value, section: *section })
            } else { None }
        })
    };
    match ea {
        EffectiveAddress::Immediate(rpn) => {
            if let Ok(v) = eval_rpn(rpn, 0, 0, 0, &lookup) {
                if v.section == 0 {
                    return EffectiveAddress::Immediate(
                        vec![RPNToken::Value(v.value as u32), RPNToken::End]);
                }
            }
            ea.clone()
        }
        EffectiveAddress::AbsLong(rpn) => {
            if let Ok(v) = eval_rpn(rpn, 0, 0, 0, &lookup) {
                if v.section == 0 {
                    return EffectiveAddress::AbsShort(
                        vec![RPNToken::Value(v.value as u32), RPNToken::End]);
                }
            }
            ea.clone()
        }
        EffectiveAddress::AbsShort(rpn) => {
            if let Ok(v) = eval_rpn(rpn, 0, 0, 0, &lookup) {
                if v.section == 0 {
                    return EffectiveAddress::AbsShort(
                        vec![RPNToken::Value(v.value as u32), RPNToken::End]);
                }
            }
            ea.clone()
        }
        EffectiveAddress::AddrRegDisp { an, disp } => {
            if let Ok(v) = eval_rpn(&disp.rpn, 0, 0, 0, &lookup) {
                if v.section == 0 {
                    // no_null_disp: displacement=0 の最適化を抑制するため明示的サイズを設定
                    let size = if no_null_disp && v.value == 0 && disp.size.is_none() {
                        Some(crate::addressing::DispSize::Word)
                    } else {
                        disp.size
                    };
                    return EffectiveAddress::AddrRegDisp {
                        an: *an,
                        disp: Displacement {
                            rpn: vec![RPNToken::Value(v.value as u32), RPNToken::End],
                            size,
                            const_val: Some(v.value),
                        },
                    };
                }
            }
            ea.clone()
        }
        _ => ea.clone(),
    }
}

fn ea_has_dynamic_ref(ea: &EffectiveAddress, sym: &SymbolTable) -> bool {
    match ea {
        EffectiveAddress::Immediate(rpn)
        | EffectiveAddress::AbsShort(rpn)
        | EffectiveAddress::AbsLong(rpn) => rpn_has_dynamic_ref(rpn, sym),
        EffectiveAddress::AddrRegDisp { disp, .. }
        | EffectiveAddress::PcDisp(disp) => rpn_has_dynamic_ref(&disp.rpn, sym),
        EffectiveAddress::AddrRegIdx { disp, .. }
        | EffectiveAddress::PcIdx { disp, .. } => rpn_has_dynamic_ref(&disp.rpn, sym),
        EffectiveAddress::MemIndPost { bd, od, .. }
        | EffectiveAddress::MemIndPre { bd, od, .. }
        | EffectiveAddress::PcMemIndPost { bd, od, .. }
        | EffectiveAddress::PcMemIndPre { bd, od, .. } => {
            rpn_has_dynamic_ref(&bd.rpn, sym) || rpn_has_dynamic_ref(&od.rpn, sym)
        }
        _ => false,
    }
}

fn rpn_has_dynamic_ref(rpn: &Rpn, sym: &SymbolTable) -> bool {
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

/// EA の拡張ワードバイト数（おおよその見積もり）
fn ea_ext_words(ea: &EffectiveAddress) -> u32 {
    match ea {
        EffectiveAddress::DataReg(_) | EffectiveAddress::AddrReg(_)
        | EffectiveAddress::AddrRegInd(_) | EffectiveAddress::AddrRegPostInc(_)
        | EffectiveAddress::AddrRegPreDec(_) => 0,
        EffectiveAddress::AbsShort(_) | EffectiveAddress::AddrRegDisp { .. }
        | EffectiveAddress::PcDisp(_) => 2,
        EffectiveAddress::AbsLong(_) => 4,
        EffectiveAddress::Immediate(rpn) => {
            // デフォルト: ワード
            let _ = rpn;
            2
        }
        EffectiveAddress::AddrRegIdx { .. } | EffectiveAddress::PcIdx { .. } => 2,
        EffectiveAddress::MemIndPost { .. } | EffectiveAddress::MemIndPre { .. }
        | EffectiveAddress::PcMemIndPost { .. } | EffectiveAddress::PcMemIndPre { .. } => 6,
        EffectiveAddress::CcrReg | EffectiveAddress::SrReg
        | EffectiveAddress::FpReg(_) | EffectiveAddress::FpCtrlReg(_) => 0,
    }
}

/// EA 内のシンボル参照を定数に置換したコピーを返す（命令バイト数推定用）
fn placeholder_ea(ea: &EffectiveAddress) -> EffectiveAddress {
    use crate::addressing::Displacement;
    // 即値は 1 を使う。0 だと SUBQ/ADDQ の範囲チェック (1-8) に引っかかるため。
    let one_rpn = || vec![RPNToken::Value(1), RPNToken::End];
    let zero_rpn = || vec![RPNToken::Value(0), RPNToken::End];
    match ea {
        EffectiveAddress::Immediate(_) => EffectiveAddress::Immediate(one_rpn()),
        EffectiveAddress::AbsShort(_)  => EffectiveAddress::AbsShort(zero_rpn()),
        EffectiveAddress::AbsLong(_)   => EffectiveAddress::AbsLong(zero_rpn()),
        EffectiveAddress::AddrRegDisp { an, disp } if disp.const_val.is_none() => {
            // ディスプレースメントが未確定（外部参照など）の場合、非ゼロのプレースホルダーを使用。
            // ゼロを使うと (0,An)→(An) 最適化が誤って適用されてしまうため。
            EffectiveAddress::AddrRegDisp {
                an: *an,
                disp: Displacement { rpn: one_rpn(), size: disp.size, const_val: Some(1) },
            }
        }
        EffectiveAddress::PcDisp(disp) if disp.const_val.is_none() => {
            EffectiveAddress::PcDisp(
                Displacement { rpn: one_rpn(), size: disp.size, const_val: Some(1) }
            )
        }
        EffectiveAddress::MemIndPost { an, bd, idx, od } => {
            EffectiveAddress::MemIndPost {
                an: *an, idx: idx.clone(),
                bd: Displacement { rpn: if bd.const_val.is_some() { bd.rpn.clone() } else { one_rpn() }, size: bd.size, const_val: bd.const_val.or(Some(1)) },
                od: Displacement { rpn: if od.const_val.is_some() { od.rpn.clone() } else { zero_rpn() }, size: od.size, const_val: od.const_val.or(Some(0)) },
            }
        }
        EffectiveAddress::MemIndPre { an, bd, idx, od } => {
            EffectiveAddress::MemIndPre {
                an: *an, idx: idx.clone(),
                bd: Displacement { rpn: if bd.const_val.is_some() { bd.rpn.clone() } else { one_rpn() }, size: bd.size, const_val: bd.const_val.or(Some(1)) },
                od: Displacement { rpn: if od.const_val.is_some() { od.rpn.clone() } else { zero_rpn() }, size: od.size, const_val: od.const_val.or(Some(0)) },
            }
        }
        EffectiveAddress::PcMemIndPost { bd, idx, od } => {
            EffectiveAddress::PcMemIndPost {
                idx: idx.clone(),
                bd: Displacement { rpn: if bd.const_val.is_some() { bd.rpn.clone() } else { one_rpn() }, size: bd.size, const_val: bd.const_val.or(Some(1)) },
                od: Displacement { rpn: if od.const_val.is_some() { od.rpn.clone() } else { zero_rpn() }, size: od.size, const_val: od.const_val.or(Some(0)) },
            }
        }
        EffectiveAddress::PcMemIndPre { bd, idx, od } => {
            EffectiveAddress::PcMemIndPre {
                idx: idx.clone(),
                bd: Displacement { rpn: if bd.const_val.is_some() { bd.rpn.clone() } else { one_rpn() }, size: bd.size, const_val: bd.const_val.or(Some(1)) },
                od: Displacement { rpn: if od.const_val.is_some() { od.rpn.clone() } else { zero_rpn() }, size: od.size, const_val: od.const_val.or(Some(0)) },
            }
        }
        other => other.clone(),
    }
}

// ----------------------------------------------------------------
// 疑似命令処理
// ----------------------------------------------------------------

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
            // delegate to pseudo/data module
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
            // ラベルが直前にある場合
            if let Some(ref name) = label {
                records.push(TempRecord::XDef { name: name.clone() });
                // シンボルの ext_attrib を更新
                if let Some(Symbol::Value { ext_attrib, .. }) = p1.sym.lookup_sym_mut(name) {
                    *ext_attrib = ExtAttrib::XDef;
                }
            }
            // オペランドに名前リストがある場合
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
            // `.page` / `.page +` は改ページ要求、`.page <expr>` は行数設定
            skip_spaces(line, pos);
            if *pos >= line.len() || line[*pos] == b';' {
                // 改ページのみ（値変更なし）
            } else if line[*pos] == b'+' {
                // `.page +`（値変更なし）
            } else {
                match parse_expr(line, pos).ok().and_then(|rpn| p1.eval_const(&rpn).map(|v| v.value)) {
                    Some(v) if v < 0 => {
                        // HAS互換: 負値は -1（自動改ページ無効）として扱う
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


        // ---- .endm / .exitm / .local / .sizem（マクロ外では無視）----
        InsnHandler::EndM | InsnHandler::ExitM | InsnHandler::Local | InsnHandler::SizeM => {}

        // ---- SCD デバッグ（MS6）----
        InsnHandler::FileScd | InsnHandler::Def | InsnHandler::Endef | InsnHandler::Val | InsnHandler::Scl
        | InsnHandler::TypeScd | InsnHandler::Tag | InsnHandler::Ln | InsnHandler::Line
        | InsnHandler::SizeScd | InsnHandler::Dim => {
            pseudo::debug::handle_scd(handler, line, pos, p1, records);
        }

        // ---- .reg ----
        InsnHandler::Reg => {
            // レジスタリスト / エイリアスシンボルの定義
            // 例: SAVED_REGS reg d3-d7/a2-a6 → レジスタマスク ValueWord として保存
            // 例: CRLF reg CR,LF → {CR の RPN, LF の RPN}
            // 例: abswarn reg abswarn2 → {abswarn2 への SymbolRef の RPN}
            // HAS互換: 単一シンボルエイリアスの場合はターゲットを即座に XREF 登録する
            skip_spaces(line, pos);
            if let Some(ref name) = label {
                let saved_pos = *pos;
                // まずレジスタリスト（MOVEM 用）として解析を試みる
                let reg_mask = parse_reg_list_mask(line, pos, p1.sym, p1.ctx.cpu_type);
                let rpns: Vec<Rpn> = if let Some(mask) = reg_mask {
                    // レジスタリスト → 定数マスクとして保存
                    vec![vec![RPNToken::ValueWord(mask), RPNToken::End]]
                } else {
                    // レジスタリストでなければ式リスト（カンマ区切り）として解析
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
                    // 単一シンボルエイリアス: [SymbolRef(target), End] → target を XREF 予約
                    if list.len() == 1 {
                        if let [RPNToken::SymbolRef(target), RPNToken::End] = list[0].as_slice() {
                            let target = target.clone();
                            // ターゲットが未定義（外部）の場合のみ XREF として登録
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
            // 最低限の互換挙動:
            // - .offsym <expr>        : .offset <expr> と同等
            // - .offsym <expr>,<sym>  : オフセット開始 + シンボルへ初期値を与える
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

        // ---- FP 等（未実装）----
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
                // 負値は FPU 命令を禁止
                p1.ctx.cpu_type &= !cpuconst::CFPP;
            } else if value <= 7 {
                p1.ctx.fpid = value as u8;
            } else {
                p1.error_code(ErrorCode::IlValue, None);
            }
        }
        InsnHandler::Pragma => {}

        _ => {} // その他未実装
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

// ----------------------------------------------------------------
// .align ヘルパー
// ----------------------------------------------------------------

/// .align n の n 値 (2^n バイト境界 → n) を解析
pub(crate) fn parse_align_n(line: &[u8], pos: &mut usize, p1: &mut P1Ctx<'_>) -> Option<u8> {
    if let Ok(rpn) = parse_expr(line, pos) {
        if let Some(v) = p1.eval_const(&rpn) {
            let align = v.value as u32;
            if align >= 2 {
                // 2^n を計算
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


// ----------------------------------------------------------------
// ユーティリティ
// ----------------------------------------------------------------

/// 識別子を読む
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

/// 文字列リテラル（"..." または 'bare'）またはそのまま識別子を読む
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

/// ファイル名を読む（引用符付きまたは空白/セミコロン区切りのパス）
/// `/tmp/foo.s` のような絶対パスや相対パスをサポートする
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
        // 空白・セミコロンまで読む
        let start = *pos;
        while *pos < line.len() {
            let b = line[*pos];
            if b == b' ' || b == b'\t' || b == b';' { break; }
            *pos += 1;
        }
        line[start..*pos].to_vec()
    }
}

/// PRN用文字列（.title/.subttl）を読む。
/// 先頭空白を飛ばし、引用符付きなら中身、そうでなければ行末/コメント手前まで。
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

// ----------------------------------------------------------------
// マクロ処理ヘルパー
// ----------------------------------------------------------------

/// .macro の仮引数リストを解析する（カンマ区切り識別子）
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

/// マクロ実引数リストを解析する（カンマ区切り、< > や ' ' で囲まれた引数をサポート）
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

/// 一つの実引数を読む（< > で囲まれた引数はその中身）
fn parse_one_macro_arg(line: &[u8], pos: &mut usize) -> Vec<u8> {
    if *pos >= line.len() { return Vec::new(); }
    if line[*pos] == b'<' {
        // <...> 形式：ネストをサポート
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
        // 通常：コンマ・セミコロン・改行まで
        let start = *pos;
        while *pos < line.len() {
            let b = line[*pos];
            if b == b',' || b == b';' || b == b'\n' { break; }
            *pos += 1;
        }
        // 末尾の空白を除去
        let end = *pos;
        let s = &line[start..end];
        s.iter().rev().skip_while(|&&b| b == b' ' || b == b'\t').count();
        let trim_end = end - s.iter().rev().take_while(|&&b| b == b' ' || b == b'\t').count();
        line[start..trim_end].to_vec()
    }
}

/// マクロボディを収集する（.endm / EOF まで）
///
/// ネストした .macro/.rept/.irp/.irpc も処理する。
/// 返値: (template バイト列, ローカルラベル数)
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
        // 末尾の CR/LF を除去
        let trim_len = line.iter().rev().take_while(|&&b| b == b'\r' || b == b'\n').count();
        let line = &line[..line.len() - trim_len];

        // ニーモニックを解析してネスト深度を調整
        let mnem = extract_mnemonic(line);
        let handler = sym.lookup_cmd(&mnem, ctx.cpu_type)
            .and_then(|s| if let Symbol::Opcode { handler, .. } = s { Some(*handler) } else { None });

        match handler {
            Some(InsnHandler::MacroDef | InsnHandler::Rept | InsnHandler::Irp | InsnHandler::Irpc) => {
                // ネストした定義
                nest_depth += 1;
                template.extend_from_slice(line);
                template.push(b'\n');
            }
            Some(InsnHandler::EndM) => {
                if nest_depth == 0 {
                    // 対応する .endm → 収集完了
                    break;
                }
                nest_depth -= 1;
                template.extend_from_slice(line);
                template.push(b'\n');
            }
            _ => {
                // 通常行: 仮引数置換と @ローカルラベル置換を保存時に行う
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

/// 行中の仮引数名を `\xFF <idx_hi> <idx_lo>` マーカーに変換する
///
/// `&param` (MASM スタイル) と裸の仮引数名 (HAS060X ネイティブスタイル) の
/// 両方を置換する。ただし '.' の直後の識別子はサイズサフィックスなので置換しない。
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
        // コメント
        if b == b';' {
            out.extend_from_slice(&line[i..]);
            break;
        }
        // '&' → 仮引数の参照 or '&&' → '&'
        if b == b'&' {
            i += 1;
            if i < line.len() && line[i] == b'&' {
                out.push(b'&');
                i += 1;
                continue;
            }
            // &param_name
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
        // '@' → ローカルラベル置換（マクロ定義中）
        // 同じ名前の @name は常に同じ lno に対応する（name_map で追跡）
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
            // @name をローカルラベルとして番号付きマーカーに変換
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
        // 文字列リテラル内も &param を置換する
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
        // 裸の識別子: HAS060X ネイティブスタイルの仮引数参照
        // '.' の直後 (サイズサフィックス) は置換しない
        if b.is_ascii_alphabetic() || b == b'_' {
            let prev = out.last().copied();
            let start = i;
            while i < line.len() && (line[i].is_ascii_alphanumeric() || line[i] == b'_') {
                i += 1;
            }
            let name = &line[start..i];
            // サイズサフィックス ('.' の直後) や '\' の直後は置換しない
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

/// マクロテンプレートを実引数で展開し、各行を parse_line に渡す
///
/// テンプレート内の .rept/.irp/.irpc は、ファイルソースではなく
/// テンプレートの残り部分からボディを収集することで正しく処理する。
pub(crate) fn expand_macro_body(
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

        // 実引数とローカルラベルを展開した行を生成
        let expanded = expand_line(tline, params, args, local_base, p1.sym);

        // .rept/.irp/.irpc はテンプレートの残り部分からボディを収集する
        // (ファイルソースを使わないことで、ネストされた .rept が正しく動作する)
        let mnem = extract_mnemonic(&expanded);
        let handler_opt = p1.sym.lookup_cmd(&mnem, p1.cpu_type())
            .and_then(|s| if let Symbol::Opcode { handler, .. } = s { Some(*handler) } else { None });

        match handler_opt {
            Some(InsnHandler::Rept) => {
                // テンプレートの残り部分からボディを収集（ファイルソース不使用）
                let remaining = &template[next_start..];
                let (body, _, consumed) = collect_body_from_slice(remaining, p1.sym, p1.ctx);
                start = next_start + consumed;

                if !p1.is_skip {
                    let line = &expanded;
                    let mut pos = 0usize;
                    skip_spaces(line, &mut pos);
                    // ニーモニック+サイズをスキップ
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
                // ボディをパラメータ変換付きで収集
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
                // 通常の行: parse_line に委譲
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

/// テンプレートスライスからボディを収集する（ファイルソース不使用版）
///
/// .rept/.irp/.irpc のボディを、ファイルソースではなくテンプレートスライスから収集する。
/// Returns (body_bytes, local_count, bytes_consumed_from_slice)
fn collect_body_from_slice(
    slice: &[u8],
    sym: &SymbolTable,
    ctx: &AssemblyContext,
) -> (Vec<u8>, u16, usize) {
    collect_body_from_slice_impl(slice, sym, ctx, &[], true)
}

/// パラメータ変換付きのスライスからボディ収集（.irp/.irpc 用）
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
        let handler = sym.lookup_cmd(&mnem, ctx.cpu_type)
            .and_then(|s| if let Symbol::Opcode { handler, .. } = s { Some(*handler) } else { None });

        match handler {
            Some(InsnHandler::MacroDef | InsnHandler::Rept | InsnHandler::Irp | InsnHandler::Irpc) => {
                nest_depth += 1;
                body.extend_from_slice(line);
                body.push(b'\n');
            }
            Some(InsnHandler::EndM) => {
                if nest_depth == 0 {
                    pos = next_pos; // .endm を消費
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

/// テンプレート行中の `\xFF idx` マーカー・`\xFE idx` マーカーを実引数に展開する
/// また `%SYMNAME` パターンをシンボルの10進数値に展開する（HAS060X互換）
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
            // ローカルラベル: ??{local_base:04X}{lno:04X} 形式
            let label = format!("??{:04X}{:04X}", local_base & 0xFFFF, lno & 0xFFFF);
            out.extend_from_slice(label.as_bytes());
        } else if b == b'%' {
            // %SYMNAME → シンボル値を10進文字列に展開（HAS060X互換）
            // 文字列リテラルや文字定数の中でも展開する（オリジナルと同様）
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
            // シンボルが見つからない場合は '%' をそのまま出力
            out.push(b);
            i += 1;
        } else {
            out.push(b);
            i += 1;
        }
    }
    out
}

/// 行のニーモニック部分だけを抽出する（スキップ判定用）
fn extract_mnemonic(line: &[u8]) -> Vec<u8> {
    let mut pos = 0;
    // ラベルフィールドをスキップ（行頭が非空白なら識別子をスキップ）
    if !line.is_empty() && line[0] != b' ' && line[0] != b'\t' {
        while pos < line.len() && line[pos] != b' ' && line[pos] != b'\t' && line[pos] != b';' {
            pos += 1;
        }
    }
    // 空白スキップ
    while pos < line.len() && (line[pos] == b' ' || line[pos] == b'\t') { pos += 1; }
    // '.' スキップ
    if pos < line.len() && line[pos] == b'.' { pos += 1; }
    let start = pos;
    while pos < line.len() && (line[pos].is_ascii_alphanumeric() || line[pos] == b'_') { pos += 1; }
    line[start..pos].to_ascii_lowercase()
}

// SymbolTable::lookup_sym_mut は symbol/mod.rs に実装済み

// ダミーハンドラ（未実装 If/Ifne/Ifeq エイリアス対応）
