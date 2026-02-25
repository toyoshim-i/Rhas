/// Pass 1: ソース行解析 → TempRecord 生成
///
/// オリジナルの `main.s` の pass1 ルーチンに対応。
/// ソーステキストをスキャンし、シンボルを定義しながら TempRecord 列を構築する。

use crate::addressing::{parse_ea, EffectiveAddress};
use crate::context::{AssemblyContext, Section};
use crate::error::SourcePos;
use crate::expr::{eval_rpn, parse_expr, Rpn};
use crate::expr::eval::EvalValue;
use crate::expr::rpn::RPNToken;
use crate::instructions::{encode_insn, InsnError};
use crate::options::cpu as cpuconst;
use crate::source::{ReadResult, SourceStack};
use crate::symbol::{Symbol, SymbolTable};
use crate::symbol::types::{DefAttrib, ExtAttrib, FirstDef, InsnHandler, SizeCode};
use super::temp::TempRecord;

// ----------------------------------------------------------------
// エラー型
// ----------------------------------------------------------------

/// Pass1 エラー
#[derive(Debug)]
pub enum Pass1Error {
    /// .include でファイルが見つからない
    IncludeNotFound(Vec<u8>),
    /// ネストが深すぎる（.include / .if 等）
    TooDeep,
    /// 致命的エラー（.fail）
    Fatal(Vec<u8>),
    /// IO エラー
    Io(std::io::Error),
}

// ----------------------------------------------------------------
// Pass1 コンテキスト
// ----------------------------------------------------------------

/// Pass1 の作業状態
struct P1Ctx<'a> {
    sym:      &'a mut SymbolTable,
    ctx:      &'a mut AssemblyContext,
    /// .if ネスト深度（最大 64 段）
    if_nest:  u16,
    /// スキップ中の .if ネスト深度（0 = スキップしていない）
    skip_nest: u16,
    /// スキップ中（is_if_skip）
    is_skip:  bool,
    /// .end が来たか
    is_end:   bool,
    /// ローカルラベルベース（マクロ展開番号用、将来実装）
    local_base: u32,
    /// 現在処理中のソース位置（エラーメッセージ用）
    current_pos: SourcePos,
}

impl<'a> P1Ctx<'a> {
    fn new(sym: &'a mut SymbolTable, ctx: &'a mut AssemblyContext) -> Self {
        P1Ctx {
            sym, ctx,
            if_nest: 0,
            skip_nest: 0,
            is_skip: false,
            is_end: false,
            local_base: 0,
            current_pos: SourcePos::new(Vec::new(), 0),
        }
    }

    /// エラーを報告して count を増やす
    fn error(&mut self, msg: &str) {
        eprintln!("{:<16} {:6}: Error: {}",
            String::from_utf8_lossy(&self.current_pos.filename),
            self.current_pos.line,
            msg);
        self.ctx.num_errors += 1;
    }

    fn section_id(&self) -> u8 { self.ctx.section as u8 }
    fn cpu_type(&self)   -> u16 { self.ctx.cpu_type }
    fn location(&self)   -> u32 { self.ctx.location() }

    fn advance(&mut self, n: u32) {
        self.ctx.advance_location(n);
    }

    fn set_section(&mut self, sec: Section) {
        self.ctx.set_section(sec);
    }

    fn set_location(&mut self, v: u32) {
        let idx = self.ctx.section as usize - 1;
        self.ctx.loc_ctr[idx] = v;
        self.ctx.loc_top = v;
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
    fn eval_const(&self, rpn: &Rpn) -> Option<EvalValue> {
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
                records.push(TempRecord::SectChange { id: p1.section_id() }); // sentinel
                // 実際は IncludeEnd はロケーションには影響なし
            }
            ReadResult::Line(line) => {
                if p1.ctx.opts.make_prn {
                    let line_num = source.current().line;
                    records.push(TempRecord::LineInfo { line_num, text: line.clone(), is_macro: false });
                }
                parse_line(&line, &mut records, &mut p1, source);
                if p1.is_end { break; }
            }
        }
    }

    records
}

// ----------------------------------------------------------------
// 行解析
// ----------------------------------------------------------------

fn parse_line(
    line: &[u8],
    records: &mut Vec<TempRecord>,
    p1: &mut P1Ctx<'_>,
    source: &mut SourceStack,
) {
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
    let mut label = if line[0] != b' ' && line[0] != b'\t' {
        parse_label(line, &mut pos)
    } else {
        None
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
            }
        }
        return;
    }

    // Case 1: 行頭ラベル後の ':=' → SET（例: N:=7）
    // parse_label が ':' を消費した後、次が '=' の場合
    if label.is_some() && pos < line.len() && line[pos] == b'=' {
        if !p1.is_skip {
            pos += 1; // '=' を消費
            skip_spaces(line, &mut pos);
            handle_set_assignment(label.as_ref().unwrap(), line, &mut pos, p1);
        }
        return;
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
        match h {
            Some(InsnHandler::If | InsnHandler::Iff | InsnHandler::Ifdef | InsnHandler::Ifndef) => {
                p1.if_nest += 1;
                // まだスキップ中なのでネストを増やすだけ
            }
            Some(InsnHandler::Else | InsnHandler::Elseif) => {
                if p1.skip_nest == p1.if_nest {
                    // この else は対応する if に対する反転
                    p1.is_skip = false;
                }
            }
            Some(InsnHandler::Endif) => {
                if p1.skip_nest == p1.if_nest {
                    p1.is_skip = false;
                    p1.if_nest -= 1;
                } else {
                    p1.if_nest -= 1;
                }
            }
            _ => {}
        }
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

    let is_equ = matches!(dispatch, Dispatch::Pseudo(InsnHandler::Equ | InsnHandler::Set));

    // ロケーションラベルを先に登録（.equ/.set 以外）
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
                expand_macro_body(&template, &params, &args, p1.local_base, records, p1, source);
                p1.local_base = p1.local_base.wrapping_add(1);
            }
        }
        // ---- 未知のニーモニック ----
        Dispatch::Unknown => {
            let msg = format!("命令が解釈できません: {}",
                String::from_utf8_lossy(&mnem));
            p1.error(&msg);
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
fn parse_label(line: &[u8], pos: &mut usize) -> Option<Vec<u8>> {
    let start = *pos;
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
    if *pos < line.len() && line[*pos] == b':' { *pos += 1; }
    Some(name)
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

fn skip_spaces(line: &[u8], pos: &mut usize) {
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
    let sz = size.unwrap_or(SizeCode::None);
    let cpu = p1.cpu_type();

    // 分岐命令（ターゲットを RPN として保持）
    if matches!(handler, InsnHandler::Bcc | InsnHandler::JBcc) {
        let target = parse_branch_target(line, pos);
        if let Some(rpn) = target {
            let byte_sz = super::temp::branch_word_size(size);
            p1.advance(byte_sz);
            records.push(TempRecord::Branch { opcode, target: rpn, req_size: size });
        } else {
            // オペランドなし (NOP/RTS 等)
            if let Ok(bytes) = encode_insn(opcode, handler, sz, &[]) {
                p1.advance(bytes.len() as u32);
                records.push(TempRecord::Const(bytes));
            }
        }
        return;
    }

    // DBcc: ターゲットを RPN として保持
    if matches!(handler, InsnHandler::DBcc) {
        let ops = parse_operands(line, pos, p1.sym, cpu);
        if ops.len() == 2 {
            // ops[0] = Dn, ops[1] = label (as AbsLong RPN)
            let dn = ops[0].clone();
            let target = if let EffectiveAddress::AbsLong(rpn) = &ops[1] {
                rpn.clone()
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

    // 通常命令
    let ops = parse_operands(line, pos, &*p1.sym, cpu);
    match encode_insn(opcode, handler, sz, &ops) {
        Ok(bytes) => {
            p1.advance(bytes.len() as u32);
            records.push(TempRecord::Const(bytes));
        }
        Err(InsnError::DeferToLinker) => {
            // シンボル参照あり → プレースホルダでサイズ推定
            let byte_size = estimate_insn_size(opcode, handler, sz, &ops);
            p1.advance(byte_size);
            records.push(TempRecord::DeferredInsn {
                base: opcode, handler, size: sz, ops, byte_size,
            });
        }
        Err(e) => {
            p1.error(&format!("命令のエンコードに失敗しました: {:?}", e));
        }
    }
}

/// 分岐命令のターゲット RPN を解析する
/// オペランドがなければ None を返す（NOP/RTS 等）
fn parse_branch_target(line: &[u8], mut pos: usize) -> Option<Rpn> {
    skip_spaces(line, &mut pos);
    if pos >= line.len() || line[pos] == b';' {
        return None; // no operand
    }
    let mut p = pos;
    match parse_expr(line, &mut p) {
        Ok(rpn) => Some(rpn),
        Err(_) => None,
    }
}

/// operand → EffectiveAddress 列
fn parse_operands(
    line:     &[u8],
    mut pos:  usize,
    sym:      &SymbolTable,
    cpu_type: u16,
) -> Vec<EffectiveAddress> {
    let mut ops = Vec::new();
    skip_spaces(line, &mut pos);

    loop {
        if pos >= line.len() || line[pos] == b';' { break; }
        match parse_ea(line, &mut pos, sym, cpu_type) {
            Ok(ea) => ops.push(ea),
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
    }
}

/// EA 内のシンボル参照を 0 に置換したコピーを返す
fn placeholder_ea(ea: &EffectiveAddress) -> EffectiveAddress {
    let zero_rpn = || vec![RPNToken::Value(0), RPNToken::End];
    match ea {
        EffectiveAddress::Immediate(_) => EffectiveAddress::Immediate(zero_rpn()),
        EffectiveAddress::AbsShort(_)  => EffectiveAddress::AbsShort(zero_rpn()),
        EffectiveAddress::AbsLong(_)   => EffectiveAddress::AbsLong(zero_rpn()),
        other => other.clone(),
    }
}

// ----------------------------------------------------------------
// 疑似命令処理
// ----------------------------------------------------------------

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
        InsnHandler::TextSect => {
            p1.set_section(Section::Text);
            records.push(TempRecord::SectChange { id: 0x01 });
        }
        InsnHandler::DataSect => {
            p1.set_section(Section::Data);
            records.push(TempRecord::SectChange { id: 0x02 });
        }
        InsnHandler::BssSect => {
            p1.set_section(Section::Bss);
            records.push(TempRecord::SectChange { id: 0x03 });
        }
        InsnHandler::Stack => {
            p1.set_section(Section::Stack);
            records.push(TempRecord::SectChange { id: 0x04 });
        }
        InsnHandler::RdataSect => {
            p1.set_section(Section::Rdata);
            records.push(TempRecord::SectChange { id: 0x05 });
        }
        InsnHandler::RbssSect => {
            p1.set_section(Section::Rbss);
            records.push(TempRecord::SectChange { id: 0x06 });
        }
        InsnHandler::RstackSect => {
            p1.set_section(Section::Rstack);
            records.push(TempRecord::SectChange { id: 0x07 });
        }
        InsnHandler::RldataSect => {
            p1.set_section(Section::Rldata);
            records.push(TempRecord::SectChange { id: 0x08 });
        }
        InsnHandler::RlbssSect => {
            p1.set_section(Section::Rlbss);
            records.push(TempRecord::SectChange { id: 0x09 });
        }
        InsnHandler::RlstackSect => {
            p1.set_section(Section::Rlstack);
            records.push(TempRecord::SectChange { id: 0x0A });
        }

        // ---- .even / .quad / .align ----
        InsnHandler::Even => {
            let sec = p1.section_id();
            let pad = if sec == 0x01 { 0x4E71u16 } else { 0u16 };
            records.push(TempRecord::Align { n: 1, pad, section: sec });
        }
        InsnHandler::Quad => {
            let sec = p1.section_id();
            records.push(TempRecord::Align { n: 2, pad: 0, section: sec });
        }
        InsnHandler::Align => {
            skip_spaces(line, pos);
            let n = parse_align_n(line, pos, p1);
            if let Some(n) = n {
                let sec = p1.section_id();
                // パディング値（オプション）
                let pad = parse_align_pad(line, pos, p1).unwrap_or_else(|| {
                    if sec == 0x01 { 0x4E71 } else { 0 }
                });
                // max_align を更新（B204 レコード用）
                if n > p1.ctx.max_align {
                    p1.ctx.max_align = n;
                }
                records.push(TempRecord::Align { n, pad, section: sec });
            }
        }

        // ---- .dc ----
        InsnHandler::Dc => {
            let byte_size: u8 = match size {
                Some(SizeCode::Byte)   => 1,
                Some(SizeCode::Long)   => 4,
                None | Some(SizeCode::Word) => 2,
                _ => 2,
            };
            parse_dc(line, pos, byte_size, records, p1);
        }

        // ---- .ds ----
        InsnHandler::Ds => {
            let item_size: u32 = match size {
                Some(SizeCode::Byte) => 1,
                Some(SizeCode::Long) => 4,
                None | Some(SizeCode::Word) => 2,
                _ => 2,
            };
            skip_spaces(line, pos);
            if let Ok(rpn) = parse_expr(line, pos) {
                if let Some(v) = p1.eval_const(&rpn) {
                    let count = v.value as u32;
                    let byte_count = count * item_size;
                    p1.advance(byte_count);
                    records.push(TempRecord::Ds { byte_count });
                }
            }
        }

        // ---- .dcb ----
        InsnHandler::Dcb => {
            let item_size: u32 = match size {
                Some(SizeCode::Byte) => 1,
                Some(SizeCode::Long) => 4,
                None | Some(SizeCode::Word) => 2,
                _ => 2,
            };
            skip_spaces(line, pos);
            // .dcb count, fill_val
            if let Ok(count_rpn) = parse_expr(line, pos) {
                let count = p1.eval_const(&count_rpn).map(|v| v.value as u32).unwrap_or(0);
                skip_spaces(line, pos);
                let fill = if *pos < line.len() && line[*pos] == b',' {
                    *pos += 1;
                    skip_spaces(line, pos);
                    let mut fill_bytes = vec![0u8; item_size as usize];
                    if let Ok(rpn) = parse_expr(line, pos) {
                        if let Some(v) = p1.eval_const(&rpn) {
                            match item_size {
                                1 => fill_bytes[0] = v.value as u8,
                                2 => {
                                    let w = v.value as u16;
                                    fill_bytes[0] = (w >> 8) as u8;
                                    fill_bytes[1] = w as u8;
                                }
                                4 => {
                                    let l = v.value as u32;
                                    fill_bytes[0] = (l >> 24) as u8;
                                    fill_bytes[1] = (l >> 16) as u8;
                                    fill_bytes[2] = (l >> 8) as u8;
                                    fill_bytes[3] = l as u8;
                                }
                                _ => {}
                            }
                        }
                    }
                    fill_bytes
                } else {
                    vec![0u8; item_size as usize]
                };
                // 繰り返しバイト列
                let mut all = Vec::with_capacity((count * item_size) as usize);
                for _ in 0..count { all.extend_from_slice(&fill); }
                let len = all.len() as u32;
                p1.advance(len);
                records.push(TempRecord::Const(all));
            }
        }

        // ---- .equ / .set ----
        InsnHandler::Equ | InsnHandler::Set => {
            skip_spaces(line, pos);
            if let Some(ref name) = label {
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
                if let Some(s) = p1.sym.lookup_sym_mut(name) {
                    if let Symbol::Value { ext_attrib, .. } = s {
                        *ext_attrib = ExtAttrib::XDef;
                    }
                }
            }
            // オペランドに名前リストがある場合
            skip_spaces(line, pos);
            while *pos < line.len() && line[*pos] != b';' {
                let name = read_ident(line, pos);
                if name.is_empty() { break; }
                records.push(TempRecord::XDef { name: name.clone() });
                if let Some(s) = p1.sym.lookup_sym_mut(&name) {
                    if let Symbol::Value { ext_attrib, .. } = s {
                        *ext_attrib = ExtAttrib::XDef;
                    }
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
            skip_spaces(line, pos);
            while *pos < line.len() && line[*pos] != b';' {
                let name = read_ident(line, pos);
                if name.is_empty() { break; }
                records.push(TempRecord::XRef { name: name.clone() });
                // 未定義シンボルとして登録
                if p1.sym.lookup_sym(&name).is_none() {
                    let sym = Symbol::Value {
                        attrib:     DefAttrib::Undef,
                        ext_attrib: ExtAttrib::XRef,
                        section:    0xFF,
                        org_num:    0,
                        first:      FirstDef::Other,
                        opt_count:  0,
                        value:      0,
                    };
                    p1.sym.define(name, sym);
                }
                skip_spaces(line, pos);
                if *pos < line.len() && line[*pos] == b',' {
                    *pos += 1;
                    skip_spaces(line, pos);
                } else { break; }
            }
        }

        // ---- .globl ----
        InsnHandler::Globl => {
            skip_spaces(line, pos);
            while *pos < line.len() && line[*pos] != b';' {
                let name = read_ident(line, pos);
                if name.is_empty() { break; }
                records.push(TempRecord::Globl { name: name.clone() });
                skip_spaces(line, pos);
                if *pos < line.len() && line[*pos] == b',' {
                    *pos += 1;
                    skip_spaces(line, pos);
                } else { break; }
            }
        }

        // ---- .org ----
        InsnHandler::Offset => {
            // .offset は .org の変形
            skip_spaces(line, pos);
            if let Ok(rpn) = parse_expr(line, pos) {
                if let Some(v) = p1.eval_const(&rpn) {
                    let org_val = v.value as u32;
                    p1.set_location(org_val);
                    records.push(TempRecord::Org { value: org_val });
                }
            }
        }

        // ---- .if / .ifdef / .ifndef ----
        InsnHandler::If => {
            p1.if_nest += 1;
            skip_spaces(line, pos);
            let cond = if let Ok(rpn) = parse_expr(line, pos) {
                p1.eval_const(&rpn).map(|v| v.value != 0).unwrap_or(false)
            } else { false };
            if !cond {
                p1.is_skip = true;
                p1.skip_nest = p1.if_nest;
            }
        }
        InsnHandler::Iff => {
            p1.if_nest += 1;
            skip_spaces(line, pos);
            let cond = if let Ok(rpn) = parse_expr(line, pos) {
                p1.eval_const(&rpn).map(|v| v.value != 0).unwrap_or(false)
            } else { false };
            // Iff: condition is ZERO → execute
            if cond {
                p1.is_skip = true;
                p1.skip_nest = p1.if_nest;
            }
        }
        InsnHandler::Ifdef => {
            p1.if_nest += 1;
            skip_spaces(line, pos);
            let name = read_ident(line, pos);
            let defined = !name.is_empty() && p1.sym.lookup_sym(&name).is_some();
            if !defined {
                p1.is_skip = true;
                p1.skip_nest = p1.if_nest;
            }
        }
        InsnHandler::Ifndef => {
            p1.if_nest += 1;
            skip_spaces(line, pos);
            let name = read_ident(line, pos);
            let defined = !name.is_empty() && p1.sym.lookup_sym(&name).is_some();
            if defined {
                p1.is_skip = true;
                p1.skip_nest = p1.if_nest;
            }
        }
        InsnHandler::Else => {
            if p1.if_nest > 0 {
                // .else の対応する .if ブロックを完了
                p1.is_skip = true;
                p1.skip_nest = p1.if_nest;
            }
        }
        InsnHandler::Elseif => {
            // .elseif = .else + .if
            // 既に実行中のブロックは完了
            if p1.if_nest > 0 {
                p1.is_skip = true;
                p1.skip_nest = p1.if_nest;
            }
        }
        InsnHandler::Endif => {
            if p1.if_nest > 0 {
                p1.if_nest -= 1;
            }
        }

        // ---- .include ----
        InsnHandler::Include | InsnHandler::Insert => {
            skip_spaces(line, pos);
            let fname = parse_string_or_ident(line, pos);
            if !fname.is_empty() {
                let _ = source.push_include(&fname);
            }
        }

        // ---- .request ----
        InsnHandler::Request => {
            // 出力に .request ファイル名を記録 → 現時点では無視
        }

        // ---- .end ----
        InsnHandler::End => {
            records.push(TempRecord::End);
            p1.is_end = true;
        }

        // ---- .cpu / CPU 指定 ----
        InsnHandler::Cpu => {
            // .cpu <name> 形式のパラメータ処理は省略
        }
        InsnHandler::Cpu68000 => {
            p1.ctx.set_cpu(68000, cpuconst::C000);
            records.push(TempRecord::Cpu { number: 68000, cpu_type: cpuconst::C000 });
        }
        InsnHandler::Cpu68010 => {
            p1.ctx.set_cpu(68010, cpuconst::C010);
            records.push(TempRecord::Cpu { number: 68010, cpu_type: cpuconst::C010 });
        }
        InsnHandler::Cpu68020 => {
            p1.ctx.set_cpu(68020, cpuconst::C020);
            records.push(TempRecord::Cpu { number: 68020, cpu_type: cpuconst::C020 });
        }
        InsnHandler::Cpu68030 => {
            p1.ctx.set_cpu(68030, cpuconst::C030);
            records.push(TempRecord::Cpu { number: 68030, cpu_type: cpuconst::C030 });
        }
        InsnHandler::Cpu68040 => {
            p1.ctx.set_cpu(68040, cpuconst::C040);
            records.push(TempRecord::Cpu { number: 68040, cpu_type: cpuconst::C040 });
        }
        InsnHandler::Cpu68060 => {
            p1.ctx.set_cpu(68060, cpuconst::C060);
            records.push(TempRecord::Cpu { number: 68060, cpu_type: cpuconst::C060 });
        }

        // ---- リスト制御（無視）----
        InsnHandler::List | InsnHandler::Nlist | InsnHandler::Lall | InsnHandler::Sall
        | InsnHandler::Width | InsnHandler::Page | InsnHandler::Title | InsnHandler::SubTtl => {}

        // ---- .fail ----
        InsnHandler::Fail => {
            p1.error(".fail によるエラー");
        }

        // ---- .macro ----
        InsnHandler::MacroDef => {
            // マクロ名はラベルフィールドに書く
            let mac_name = label.clone().unwrap_or_else(Vec::new);
            if mac_name.is_empty() {
                p1.error(".macro にマクロ名がありません");
                return;
            }
            // 仮引数リストを解析
            let params = parse_macro_params(line, pos);
            // ボディを収集（.endm まで）
            let (template, local_count) = collect_macro_body(source, p1.sym, p1.ctx, &params);
            let sym = Symbol::Macro { params, local_count, template };
            p1.sym.define_macro(mac_name, sym);
        }

        // ---- .rept ----
        InsnHandler::Rept => {
            let count = if let Ok(rpn) = parse_expr(line, pos) {
                p1.eval_const(&rpn).map(|v| v.value as u32).unwrap_or(0)
            } else { 0 };
            let (body, _) = collect_macro_body(source, p1.sym, p1.ctx, &[]);
            for _ in 0..count {
                expand_macro_body(&body, &[], &[], p1.local_base, records, p1, source);
                p1.local_base = p1.local_base.wrapping_add(1);
            }
        }

        // ---- .irp ----
        InsnHandler::Irp => {
            // .irp param, arg1, arg2, ...
            skip_spaces(line, pos);
            let param_name = read_ident(line, pos);
            skip_spaces(line, pos);
            if *pos < line.len() && line[*pos] == b',' { *pos += 1; }
            let args = parse_macro_args(line, pos);
            let params = if param_name.is_empty() { vec![] } else { vec![param_name] };
            let (body, _) = collect_macro_body(source, p1.sym, p1.ctx, &params);
            for arg in &args {
                expand_macro_body(&body, &params, std::slice::from_ref(arg), p1.local_base, records, p1, source);
                p1.local_base = p1.local_base.wrapping_add(1);
            }
        }

        // ---- .irpc ----
        InsnHandler::Irpc => {
            // .irpc param, string
            skip_spaces(line, pos);
            let param_name = read_ident(line, pos);
            skip_spaces(line, pos);
            if *pos < line.len() && line[*pos] == b',' { *pos += 1; }
            skip_spaces(line, pos);
            // 文字列（クォートあり/なし）
            let s = parse_string_or_ident(line, pos);
            let params = if param_name.is_empty() { vec![] } else { vec![param_name] };
            let (body, _) = collect_macro_body(source, p1.sym, p1.ctx, &params);
            for &ch in &s {
                let arg = vec![ch];
                expand_macro_body(&body, &params, std::slice::from_ref(&arg), p1.local_base, records, p1, source);
                p1.local_base = p1.local_base.wrapping_add(1);
            }
        }

        // ---- .endm / .exitm / .local / .sizem（マクロ外では無視）----
        InsnHandler::EndM | InsnHandler::ExitM | InsnHandler::Local | InsnHandler::SizeM => {}

        // ---- SCD デバッグ（Phase 10）----
        InsnHandler::Def | InsnHandler::Endef | InsnHandler::Val | InsnHandler::Scl
        | InsnHandler::TypeScd | InsnHandler::Tag | InsnHandler::Line
        | InsnHandler::SizeScd | InsnHandler::Dim => {}

        // ---- .reg ----
        InsnHandler::Reg => {
            // レジスタリストシンボルの定義（Phase 8）
        }

        // ---- .comm / .rcomm / .rlcomm ----
        InsnHandler::Comm | InsnHandler::Rcomm | InsnHandler::Rlcomm => {}

        // ---- .offsym ----
        InsnHandler::OffsymPs => {}

        // ---- FP 等（未実装）----
        InsnHandler::FpId | InsnHandler::Pragma => {}

        _ => {} // その他未実装
    }
}

// ----------------------------------------------------------------
// .dc 解析
// ----------------------------------------------------------------

fn parse_dc(
    line:      &[u8],
    pos:       &mut usize,
    byte_size: u8,
    records:   &mut Vec<TempRecord>,
    p1:        &mut P1Ctx<'_>,
) {
    skip_spaces(line, pos);
    loop {
        if *pos >= line.len() || line[*pos] == b';' { break; }

        // 文字列リテラル "..." → バイト列として埋め込む
        if line[*pos] == b'"' {
            *pos += 1;
            let mut s = Vec::new();
            while *pos < line.len() && line[*pos] != b'"' {
                s.push(line[*pos]);
                *pos += 1;
            }
            if *pos < line.len() { *pos += 1; } // closing "
            // byte_size == 1 なら各バイト, word → 2バイト並び
            match byte_size {
                1 => {
                    p1.advance(s.len() as u32);
                    records.push(TempRecord::Const(s));
                }
                2 => {
                    // 各文字を 2 バイトワード（上位ゼロ）で埋め込む
                    let mut bytes = Vec::with_capacity(s.len() * 2);
                    for b in &s { bytes.push(0); bytes.push(*b); }
                    p1.advance(bytes.len() as u32);
                    records.push(TempRecord::Const(bytes));
                }
                4 => {
                    let mut bytes = Vec::with_capacity(s.len() * 4);
                    for b in &s { bytes.push(0); bytes.push(0); bytes.push(0); bytes.push(*b); }
                    p1.advance(bytes.len() as u32);
                    records.push(TempRecord::Const(bytes));
                }
                _ => {}
            }
        } else {
            // 式
            match parse_expr(line, pos) {
                Ok(rpn) => {
                    if let Some(v) = p1.eval_const(&rpn) {
                        // 定数 → Const
                        let bytes = val_to_bytes(v.value, byte_size);
                        p1.advance(bytes.len() as u32);
                        records.push(TempRecord::Const(bytes));
                    } else {
                        // 未解決 → Data
                        p1.advance(byte_size as u32);
                        records.push(TempRecord::Data { size: byte_size, rpn });
                    }
                }
                Err(_) => break,
            }
        }

        // カンマ区切り
        skip_spaces(line, pos);
        if *pos < line.len() && line[*pos] == b',' {
            *pos += 1;
            skip_spaces(line, pos);
        } else {
            break;
        }
    }
}

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

// ----------------------------------------------------------------
// .align ヘルパー
// ----------------------------------------------------------------

/// .align n の n 値 (2^n バイト境界 → n) を解析
fn parse_align_n(line: &[u8], pos: &mut usize, p1: &mut P1Ctx<'_>) -> Option<u8> {
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

fn parse_align_pad(line: &[u8], pos: &mut usize, p1: &mut P1Ctx<'_>) -> Option<u16> {
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
fn read_ident(line: &[u8], pos: &mut usize) -> Vec<u8> {
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
fn parse_string_or_ident(line: &[u8], pos: &mut usize) -> Vec<u8> {
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

// ----------------------------------------------------------------
// マクロ処理ヘルパー
// ----------------------------------------------------------------

/// .macro の仮引数リストを解析する（カンマ区切り識別子）
fn parse_macro_params(line: &[u8], pos: &mut usize) -> Vec<Vec<u8>> {
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
fn parse_macro_args(line: &[u8], pos: &mut usize) -> Vec<Vec<u8>> {
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
fn collect_macro_body(
    source: &mut SourceStack,
    sym:    &SymbolTable,
    ctx:    &mut AssemblyContext,
    params: &[Vec<u8>],
) -> (Vec<u8>, u16) {
    let mut template = Vec::new();
    let mut local_count = 0u16;
    let mut nest_depth = 0u32;

    loop {
        let line = match source.read_line() {
            ReadResult::Line(l) => l,
            ReadResult::Eof | ReadResult::IncludeEnd => break,
        };
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
                // 通常行: 仮引数を `\xFF param_idx` マーカーに変換して保存
                if nest_depth == 0 && !params.is_empty() {
                    let converted = convert_line_params(line, params, &mut local_count);
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
fn convert_line_params(line: &[u8], params: &[Vec<u8>], local_count: &mut u16) -> Vec<u8> {
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
                p.len() == name.len() && p.iter().zip(name).all(|(a,b)| a.to_ascii_lowercase() == b.to_ascii_lowercase())
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
        if b == b'@' && i + 1 < line.len() && line[i+1] != b'@' {
            // @name をローカルラベルとして番号付きマーカーに変換
            i += 1;
            let start = i;
            while i < line.len() && (line[i].is_ascii_alphanumeric() || line[i] == b'_') {
                i += 1;
            }
            let _name = &line[start..i];
            let lno = *local_count;
            *local_count += 1;
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
                        p.len() == name.len() && p.iter().zip(name).all(|(a,b2)| a.to_ascii_lowercase() == b2.to_ascii_lowercase())
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
            // サイズサフィックス ('.' の直後) でなければ仮引数をチェック
            if prev != Some(b'.') {
                if let Some(idx) = params.iter().position(|p| {
                    p.len() == name.len() && p.iter().zip(name.iter()).all(|(a, b2)| {
                        a.to_ascii_lowercase() == b2.to_ascii_lowercase()
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
fn expand_macro_body(
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
                        expand_macro_body(&body, &[], &[], p1.local_base, records, p1, source);
                        p1.local_base = p1.local_base.wrapping_add(1);
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
                        expand_macro_body(&body, &irp_params,
                            std::slice::from_ref(irp_arg), p1.local_base, records, p1, source);
                        p1.local_base = p1.local_base.wrapping_add(1);
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
                        expand_macro_body(&body, &irpc_params,
                            std::slice::from_ref(&arg), p1.local_base, records, p1, source);
                        p1.local_base = p1.local_base.wrapping_add(1);
                    }
                }
                continue;
            }
            _ => {
                // 通常の行: parse_line に委譲
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
    collect_body_from_slice_impl(slice, sym, ctx, &[], false)
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
                if do_param_convert && !params.is_empty() {
                    let converted = convert_line_params(line, params, &mut local_count);
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

// ----------------------------------------------------------------
// SymbolTable の可変参照（lookup_sym_mut が必要）
// ----------------------------------------------------------------
// SymbolTable に lookup_sym_mut を追加するか、ここで直接アクセスする
// 現時点では define() で上書き定義する
trait SymbolTableExt {
    fn lookup_sym_mut(&mut self, name: &[u8]) -> Option<&mut Symbol>;
}

impl SymbolTableExt for SymbolTable {
    fn lookup_sym_mut(&mut self, _name: &[u8]) -> Option<&mut Symbol> {
        // SymbolTable に内部 HashMap へのアクセスが必要
        // 現時点では None を返す（ext_attrib 更新を後回し）
        None
    }
}

// ダミーハンドラ（未実装 If/Ifne/Ifeq エイリアス対応）
trait InsnHandlerAlias {}
