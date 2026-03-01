// 原典（expr.s）由来の定義を先行移植しているため
// 未参照の項目が残っている。
#![allow(dead_code)]
/// 式パーサー（ソーステキスト → 逆ポーランド式）
///
/// オリジナルの `convrpn`（expr.s）と字句解析部分に対応する。
/// Rust版はソーステキストから直接 RPN トークン列を生成する。
/// シャンティングヤードアルゴリズムを使用する。

pub mod rpn;
pub mod eval;

#[allow(unused_imports)]
pub use rpn::{Operator, RPNToken, Rpn};
#[allow(unused_imports)]
pub use eval::{EvalError, EvalValue, eval_rpn};

use rpn::Operator as Op;

// ----------------------------------------------------------------
// エラー型
// ----------------------------------------------------------------

/// 式パーサーのエラー
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// 式が見つからない（空または予期しない文字）
    ExprExpected,
    /// 閉じ括弧がない
    UnclosedParen,
    /// 閉じ括弧が多すぎる
    UnexpectedCloseParen,
    /// 文字定数が閉じていない
    UnclosedCharConst,
    /// 数値が不正
    InvalidNumber,
    /// `>>>`（算術右シフトの別記法）サポートのための内部記号
    #[allow(dead_code)]
    Internal,
}

// ----------------------------------------------------------------
// パーサー
// ----------------------------------------------------------------

/// 式パーサー
struct Parser<'a> {
    src: &'a [u8],
    pos: usize,
    output: Vec<RPNToken>,
    /// 演算子スタック（シャンティングヤード）
    op_stack: Vec<StackItem>,
}

#[derive(Debug, Clone, Copy)]
enum StackItem {
    /// スタックの底マーカー（括弧の区切りにも使う）
    Bottom,
    /// 演算子
    Op(Op),
}

impl StackItem {
    /// スタックアイテムの優先順位
    /// Bottom は 255（決して pop されない）
    fn priority(self) -> u8 {
        match self {
            StackItem::Bottom  => u8::MAX,
            StackItem::Op(op)  => op.priority(),
        }
    }
}

impl<'a> Parser<'a> {
    fn new(src: &'a [u8]) -> Self {
        Parser {
            src,
            pos: 0,
            output: Vec::with_capacity(16),
            op_stack: vec![StackItem::Bottom],
        }
    }

    // ----------------------------------------------------------------
    // トークン読み取りユーティリティ
    // ----------------------------------------------------------------

    fn peek(&self) -> Option<u8> {
        self.src.get(self.pos).copied()
    }

    fn peek2(&self) -> Option<u8> {
        self.src.get(self.pos + 1).copied()
    }

    fn advance(&mut self) -> Option<u8> {
        let b = self.src.get(self.pos).copied();
        if b.is_some() { self.pos += 1; }
        b
    }

    fn skip_ws(&mut self) {
        while let Some(b) = self.peek() {
            if b == b' ' || b == b'\t' { self.pos += 1; } else { break; }
        }
    }

    fn try_eat(&mut self, b: u8) -> bool {
        if self.peek() == Some(b) { self.pos += 1; true } else { false }
    }

    // ----------------------------------------------------------------
    // シャンティングヤード: 演算子スタック操作
    // ----------------------------------------------------------------

    /// 演算子 op をスタックに積む前に、優先順位が高い（≤ priority）演算子を pop して出力する。
    fn shunt_binary(&mut self, op: Op) {
        let op_prio = op.priority();
        loop {
            match self.op_stack.last() {
                Some(top) if top.priority() <= op_prio => {
                    // pop して出力（Bottom は priority=255 なので条件に引っかからない）
                    if let Some(StackItem::Op(top_op)) = self.op_stack.pop() {
                        self.emit_op(top_op);
                    }
                }
                _ => break,
            }
        }
        self.op_stack.push(StackItem::Op(op));
    }

    /// スタックに Bottom マーカーを積む（括弧用）
    fn push_bottom(&mut self) {
        self.op_stack.push(StackItem::Bottom);
    }

    /// Bottom マーカーまでスタックを pop して出力する
    fn pop_to_bottom(&mut self) -> bool {
        loop {
            match self.op_stack.last() {
                None => return false,
                Some(StackItem::Bottom) => {
                    self.op_stack.pop();
                    return true;
                }
                Some(StackItem::Op(_)) => {
                    if let Some(StackItem::Op(op)) = self.op_stack.pop() {
                        self.emit_op(op);
                    }
                }
            }
        }
    }

    /// 残り全ての演算子を出力する（式終端）
    fn flush_ops(&mut self) {
        loop {
            match self.op_stack.last() {
                None | Some(StackItem::Bottom) => break,
                Some(StackItem::Op(_)) => {
                    if let Some(StackItem::Op(op)) = self.op_stack.pop() {
                        self.emit_op(op);
                    }
                }
            }
        }
    }

    fn emit_op(&mut self, op: Op) {
        self.output.push(RPNToken::Op(op));
    }

    // ----------------------------------------------------------------
    // トップレベルの式解析
    // ----------------------------------------------------------------

    fn parse(&mut self) -> Result<(), ParseError> {
        self.skip_ws();
        self.parse_primary()?;
        loop {
            self.skip_ws();
            match self.try_binary_op() {
                Some(op) => {
                    self.shunt_binary(op);
                    self.skip_ws();
                    self.parse_primary()?;
                }
                None => break,
            }
        }
        self.flush_ops();
        self.output.push(RPNToken::End);
        Ok(())
    }

    // ----------------------------------------------------------------
    // 一次式（数値・シンボル・括弧・単項演算子）
    // ----------------------------------------------------------------

    fn parse_primary(&mut self) -> Result<(), ParseError> {
        self.skip_ws();
        // 単項演算子を先読み
        if let Some(op) = self.try_unary_op() {
            // 単項演算子はスタックに積む（右結合）
            self.op_stack.push(StackItem::Op(op));
            self.skip_ws();
            self.parse_primary()?;
            // 単項演算子を出力（積まれた位置まで）
            // shunt_binary() が呼ばれるか flush_ops() で出力される
            return Ok(());
        }
        // 括弧
        if self.try_eat(b'(') {
            self.push_bottom();
            self.skip_ws();
            self.parse_primary()?;
            loop {
                self.skip_ws();
                match self.try_binary_op() {
                    Some(op) => {
                        self.shunt_binary(op);
                        self.skip_ws();
                        self.parse_primary()?;
                    }
                    None => break,
                }
            }
            if !self.pop_to_bottom() {
                return Err(ParseError::UnclosedParen);
            }
            self.skip_ws();
            if !self.try_eat(b')') {
                return Err(ParseError::UnclosedParen);
            }
            return Ok(());
        }
        // ロケーションカウンタ '*'
        if self.peek() == Some(b'*') {
            // '*' がオペランドとして使われている（演算子ではない）
            self.pos += 1;
            self.output.push(RPNToken::Location);
            return Ok(());
        }
        // '$' = 現在ロケーション OR 16進数リテラル
        if self.peek() == Some(b'$') {
            if self.peek2().map(|b| b.is_ascii_hexdigit()).unwrap_or(false) {
                // '$1234' 形式の 16 進数
                self.pos += 1; // '$' を消費
                let v = self.parse_hex_digits()?;
                self.push_value(v);
            } else {
                self.pos += 1;
                self.output.push(RPNToken::CurrentLoc);
            }
            return Ok(());
        }
        // '0x' / '0X' 形式の 16 進数
        if self.peek() == Some(b'0')
            && matches!(self.peek2(), Some(b'x') | Some(b'X'))
        {
            self.pos += 2;
            let v = self.parse_hex_digits()?;
            self.push_value(v);
            return Ok(());
        }
        // '@' 形式の 8 進数（次の文字が 0-7 の場合のみ; @@N は識別子として処理）
        if self.peek() == Some(b'@') && self.peek2().map(|b| b >= b'0' && b <= b'7').unwrap_or(false) {
            self.pos += 1;
            let v = self.parse_octal_digits()?;
            self.push_value(v);
            return Ok(());
        }
        // '%' 形式の 2 進数
        if self.peek() == Some(b'%') {
            self.pos += 1;
            let v = self.parse_binary_digits()?;
            self.push_value(v);
            return Ok(());
        }
        // 文字定数 'A' / 'AB' または "A" / "AB"
        if self.peek() == Some(b'\'') || self.peek() == Some(b'"') {
            let close = self.peek().unwrap();
            self.pos += 1;
            let v = self.parse_char_const_with_close(close)?;
            self.push_value(v);
            return Ok(());
        }
        // 10 進数
        if self.peek().map(|b| b.is_ascii_digit()).unwrap_or(false) {
            let v = self.parse_decimal()?;
            self.push_value(v);
            return Ok(());
        }
        // シンボル名 / キーワード演算子（. で始まる）
        if self.peek() == Some(b'.') {
            if let Some(op) = self.try_keyword_unary() {
                self.op_stack.push(StackItem::Op(op));
                self.skip_ws();
                return self.parse_primary();
            }
            // .defined. シンボル名 → 定数 0 or -1 に展開（後でシンボルテーブル参照）
            if self.try_keyword(b"defined") {
                return self.parse_defined();
            }
            // その他の '.' で始まるシンボル（.xxx 形式のローカルラベル等）
            if let Some(name) = self.try_ident_starting_with_dot() {
                self.output.push(RPNToken::SymbolRef(name));
                return Ok(());
            }
            return Err(ParseError::ExprExpected);
        }
        // 識別子（シンボル名）
        if self.peek().map(|b| is_ident_start(b)).unwrap_or(false) {
            let name = self.parse_ident();
            self.output.push(RPNToken::SymbolRef(name));
            return Ok(());
        }
        Err(ParseError::ExprExpected)
    }

    // ----------------------------------------------------------------
    // 数値リテラル
    // ----------------------------------------------------------------

    fn push_value(&mut self, v: u32) {
        if v <= 0xFF {
            self.output.push(RPNToken::ValueByte(v as u8));
        } else if v <= 0xFFFF {
            self.output.push(RPNToken::ValueWord(v as u16));
        } else {
            self.output.push(RPNToken::Value(v));
        }
    }

    fn parse_hex_digits(&mut self) -> Result<u32, ParseError> {
        let start = self.pos;
        while self.peek().map(|b| b.is_ascii_hexdigit() || b == b'_').unwrap_or(false) {
            self.pos += 1;
        }
        if self.pos == start { return Err(ParseError::InvalidNumber); }
        let s = &self.src[start..self.pos];
        let mut v: u32 = 0;
        for &b in s {
            if b == b'_' { continue; }  // digit separator
            let d = hex_digit(b);
            v = v.wrapping_mul(16).wrapping_add(d as u32);
        }
        Ok(v)
    }

    fn parse_octal_digits(&mut self) -> Result<u32, ParseError> {
        let start = self.pos;
        while self.peek().map(|b| (b >= b'0' && b <= b'7') || b == b'_').unwrap_or(false) {
            self.pos += 1;
        }
        if self.pos == start { return Err(ParseError::InvalidNumber); }
        let s = &self.src[start..self.pos];
        let mut v: u32 = 0;
        for &b in s {
            if b == b'_' { continue; }  // digit separator
            v = v.wrapping_mul(8).wrapping_add((b - b'0') as u32);
        }
        Ok(v)
    }

    fn parse_binary_digits(&mut self) -> Result<u32, ParseError> {
        let start = self.pos;
        while self.peek().map(|b| b == b'0' || b == b'1' || b == b'_').unwrap_or(false) {
            self.pos += 1;
        }
        if self.pos == start { return Err(ParseError::InvalidNumber); }
        let s = &self.src[start..self.pos];
        let mut v: u32 = 0;
        for &b in s {
            if b == b'_' { continue; }  // digit separator
            v = v.wrapping_mul(2).wrapping_add((b - b'0') as u32);
        }
        Ok(v)
    }

    fn parse_decimal(&mut self) -> Result<u32, ParseError> {
        let start = self.pos;
        while self.peek().map(|b| b.is_ascii_digit() || b == b'_').unwrap_or(false) {
            self.pos += 1;
        }
        if self.pos == start { return Err(ParseError::InvalidNumber); }
        let s = &self.src[start..self.pos];
        let mut v: u32 = 0;
        for &b in s {
            if b == b'_' { continue; }  // digit separator
            v = v.wrapping_mul(10).wrapping_add((b - b'0') as u32);
        }
        Ok(v)
    }

    /// 文字定数 'A' / 'AB' / 'ABC' / 'ABCD'（最大 4 文字）
    #[allow(dead_code)]
    fn parse_char_const(&mut self) -> Result<u32, ParseError> {
        self.parse_char_const_with_close(b'\'')
    }

    fn parse_char_const_with_close(&mut self, close: u8) -> Result<u32, ParseError> {
        let mut v: u32 = 0;
        let mut count = 0;
        loop {
            match self.peek() {
                None | Some(b'\n') | Some(b'\r') => return Err(ParseError::UnclosedCharConst),
                Some(b) if b == close => {
                    self.pos += 1;
                    break;
                }
                Some(b) => {
                    // Shift_JIS の 2 バイト文字を考慮：先行バイトなら次のバイトも取り込む
                    self.pos += 1;
                    count += 1;
                    if count > 4 { return Err(ParseError::InvalidNumber); }
                    v = (v << 8) | b as u32;
                    if is_sjis_lead(b) {
                        if let Some(b2) = self.peek() {
                            self.pos += 1;
                            count += 1;
                            if count > 4 { return Err(ParseError::InvalidNumber); }
                            v = (v << 8) | b2 as u32;
                        }
                    }
                }
            }
        }
        Ok(v)
    }

    // ----------------------------------------------------------------
    // 識別子
    // ----------------------------------------------------------------

    fn parse_ident(&mut self) -> Vec<u8> {
        let start = self.pos;
        while self.peek().map(|b| is_ident_cont(b)).unwrap_or(false) {
            self.pos += 1;
        }
        self.src[start..self.pos].to_vec()
    }

    fn try_ident_starting_with_dot(&mut self) -> Option<Vec<u8>> {
        if self.peek() != Some(b'.') { return None; }
        let save = self.pos;
        self.pos += 1;
        if self.peek().map(|b| is_ident_start(b)).unwrap_or(false) {
            let mut name = vec![b'.'];
            while self.peek().map(|b| is_ident_cont(b)).unwrap_or(false) {
                name.push(self.advance().unwrap());
            }
            Some(name)
        } else {
            self.pos = save;
            None
        }
    }

    // ----------------------------------------------------------------
    // 演算子
    // ----------------------------------------------------------------

    /// 単項演算子を試みる
    fn try_unary_op(&mut self) -> Option<Op> {
        match self.peek()? {
            b'-' => { self.pos += 1; Some(Op::Neg) }
            b'+' => { self.pos += 1; Some(Op::Pos) }
            b'~' => {
                // HAS互換: '~' の後にアルファベット・アンダースコア・'~' が続く場合は
                // シンボル名の一部（~symbol, ~~label）として扱う
                // それ以外（数値、'$'等）は NOT 演算子として扱う
                let next = self.peek2();
                let is_tilde_sym = next.map(|b| {
                    b.is_ascii_alphabetic() || b == b'_' || b == b'~' || b == b'?' || b == b'@'
                }).unwrap_or(false);
                if is_tilde_sym {
                    None  // シンボル名として parse_primary に任せる
                } else {
                    self.pos += 1;
                    Some(Op::Not)
                }
            }
            b'.' => self.try_keyword_unary(),
            _ => None,
        }
    }

    /// '.' で始まるキーワード単項演算子を試みる
    fn try_keyword_unary(&mut self) -> Option<Op> {
        let table: &[(&[u8], Op)] = &[
            (b"not",   Op::Not),
            (b"high",  Op::High),
            (b"low",   Op::Low),
            (b"highw", Op::HighW),
            (b"loww",  Op::LowW),
            (b"nul",   Op::Nul),
            (b"notb",  Op::Not),   // .notb. は後処理（今は .not. と同等）
            (b"notw",  Op::Not),   // .notw. も同様
        ];
        let save = self.pos;
        if self.peek() == Some(b'.') { self.pos += 1; }
        for (kw, op) in table {
            if self.src[self.pos..].starts_with(kw) {
                let end = self.pos + kw.len();
                if self.src.get(end) == Some(&b'.') {
                    self.pos = end + 1;
                    return Some(*op);
                }
            }
        }
        self.pos = save;
        None
    }

    /// キーワードを消費して true を返す（'.' prefix と suffix を含む）
    fn try_keyword(&mut self, kw: &[u8]) -> bool {
        let save = self.pos;
        if self.peek() == Some(b'.') { self.pos += 1; }
        if self.src[self.pos..].starts_with(kw) {
            let end = self.pos + kw.len();
            if self.src.get(end) == Some(&b'.') {
                self.pos = end + 1;
                return true;
            }
        }
        self.pos = save;
        false
    }

    /// 二項演算子を試みる
    fn try_binary_op(&mut self) -> Option<Op> {
        self.skip_ws();
        let b = self.peek()?;
        match b {
            b'+' => { self.pos += 1; Some(Op::Add) }
            b'-' => { self.pos += 1; Some(Op::Sub) }
            b'*' => { self.pos += 1; Some(Op::Mul) }
            b'/' => { self.pos += 1; Some(Op::Div) }
            b'&' => { self.pos += 1; Some(Op::And) }
            b'^' => { self.pos += 1; Some(Op::Xor) }
            b'|' => { self.pos += 1; Some(Op::Or)  }
            b'=' => {
                self.pos += 1;
                self.try_eat(b'='); // '==' は '=' と同じ
                Some(Op::Eq)
            }
            b'!' => {
                if self.peek2() == Some(b'=') {
                    self.pos += 2;
                    Some(Op::Ne)
                } else { None }
            }
            b'<' => {
                self.pos += 1;
                match self.peek() {
                    Some(b'<') => { self.pos += 1; Some(Op::Shl) }
                    Some(b'>') => { self.pos += 1; Some(Op::Ne)  }
                    Some(b'=') => { self.pos += 1; Some(Op::Le)  }
                    _ => Some(Op::Lt),
                }
            }
            b'>' => {
                self.pos += 1;
                match self.peek() {
                    Some(b'>') => {
                        self.pos += 1;
                        // '>>>' → .asr.（算術右シフト）
                        if self.peek() == Some(b'>') { self.pos += 1; Some(Op::Asr) }
                        else { Some(Op::Shr) }
                    }
                    Some(b'=') => { self.pos += 1; Some(Op::Ge) }
                    _ => Some(Op::Gt),
                }
            }
            b'.' => {
                // キーワード二項演算子
                self.try_keyword_binary()
            }
            _ => None,
        }
    }

    /// '.' で始まるキーワード二項演算子を試みる
    fn try_keyword_binary(&mut self) -> Option<Op> {
        let table: &[(&[u8], Op)] = &[
            (b"mod",  Op::Mod),
            (b"shr",  Op::Shr),
            (b"shl",  Op::Shl),
            (b"asr",  Op::Asr),
            (b"and",  Op::And),
            (b"eor",  Op::Xor),  // HAS の .eor. = XOR
            (b"or",   Op::Or),
            (b"xor",  Op::Xor),
            (b"eq",   Op::Eq),
            (b"ne",   Op::Ne),
            (b"slt",  Op::Slt),
            (b"sle",  Op::Sle),
            (b"sgt",  Op::Sgt),
            (b"sge",  Op::Sge),
            (b"lt",   Op::Lt),
            (b"le",   Op::Le),
            (b"gt",   Op::Gt),
            (b"ge",   Op::Ge),
        ];
        let save = self.pos;
        if self.peek() == Some(b'.') { self.pos += 1; }
        for (kw, op) in table {
            if self.src[self.pos..].starts_with(kw) {
                let end = self.pos + kw.len();
                if self.src.get(end) == Some(&b'.') {
                    self.pos = end + 1;
                    return Some(*op);
                }
            }
        }
        self.pos = save;
        None
    }

    // ----------------------------------------------------------------
    // .defined.
    // ----------------------------------------------------------------

    /// `.defined. SYMBOL` を解析して SymbolDefined トークンを生成する
    /// オリジナルでは即値として評価されるが、Rust版ではシンボル参照として
    /// eval 側で解決する。ここでは「SymbolRef に続いて Defined 演算子」の代わりに
    /// シンボル名に "?defined?" プレフィックスを付けて表現する。
    fn parse_defined(&mut self) -> Result<(), ParseError> {
        self.skip_ws();
        // .defined.(SYMBOL) または .defined. SYMBOL
        let paren = self.try_eat(b'(');
        self.skip_ws();
        let name = if self.peek().map(is_ident_start).unwrap_or(false) {
            self.parse_ident()
        } else {
            return Err(ParseError::ExprExpected);
        };
        if paren {
            self.skip_ws();
            if !self.try_eat(b')') { return Err(ParseError::UnclosedParen); }
        }
        // 特殊シンボル参照として出力（eval 時に解決する）
        let mut key = b"\x01defined\x01".to_vec();
        key.extend_from_slice(&name);
        self.output.push(RPNToken::SymbolRef(key));
        Ok(())
    }
}

// ----------------------------------------------------------------
// 公開 API
// ----------------------------------------------------------------

/// ソーステキストから逆ポーランド式に変換する
///
/// * `src`     - ソースバイト列
/// * `pos`     - 開始位置（変換後に消費したバイト数を反映する）
///
/// `parse_expr(src, &mut pos)` で式を解析し、`eval_rpn` で評価する。
pub fn parse_expr(src: &[u8], pos: &mut usize) -> Result<Rpn, ParseError> {
    let mut parser = Parser::new(&src[*pos..]);
    parser.parse()?;
    *pos += parser.pos;
    Ok(parser.output)
}

// ----------------------------------------------------------------
// ユーティリティ
// ----------------------------------------------------------------

fn is_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_' || b == b'.' || b == b'?' || b == b'@' || b == b'~'
}

fn is_ident_cont(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'$' || b == b'?' || b == b'@' || b == b'~'
}

fn hex_digit(b: u8) -> u8 {
    if b.is_ascii_digit() { b - b'0' }
    else if b >= b'a' { b - b'a' + 10 }
    else { b - b'A' + 10 }
}

/// Shift_JIS 先行バイト判定
fn is_sjis_lead(b: u8) -> bool {
    (b >= 0x81 && b <= 0x9F) || (b >= 0xE0 && b <= 0xFC)
}

// ----------------------------------------------------------------
// テスト
// ----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> Result<Rpn, ParseError> {
        let mut pos = 0;
        parse_expr(s.as_bytes(), &mut pos)
    }

    fn eval_const(rpn: Rpn) -> i32 {
        eval_rpn(&rpn, 0, 0, 0, &|_| None).unwrap().value
    }

    #[test]
    fn test_decimal() {
        let rpn = parse("42").unwrap();
        assert_eq!(eval_const(rpn), 42);
    }

    #[test]
    fn test_hex() {
        assert_eq!(eval_const(parse("$1A").unwrap()), 0x1A);
        assert_eq!(eval_const(parse("0xFF").unwrap()), 0xFF);
    }

    #[test]
    fn test_octal() {
        assert_eq!(eval_const(parse("@17").unwrap()), 0o17);
    }

    #[test]
    fn test_binary() {
        assert_eq!(eval_const(parse("%1010").unwrap()), 10);
    }

    #[test]
    fn test_char_const() {
        assert_eq!(eval_const(parse("'A'").unwrap()), b'A' as i32);
        assert_eq!(eval_const(parse("'AB'").unwrap()), ((b'A' as i32) << 8) | b'B' as i32);
    }

    #[test]
    fn test_add() {
        assert_eq!(eval_const(parse("1+2").unwrap()), 3);
        assert_eq!(eval_const(parse("10 - 3").unwrap()), 7);
    }

    #[test]
    fn test_mul_prec() {
        // 2 + 3 * 4 = 14（乗算が先）
        assert_eq!(eval_const(parse("2+3*4").unwrap()), 14);
        // (2+3)*4 = 20
        assert_eq!(eval_const(parse("(2+3)*4").unwrap()), 20);
    }

    #[test]
    fn test_unary_neg() {
        assert_eq!(eval_const(parse("-5").unwrap()), -5);
        assert_eq!(eval_const(parse("-(3+4)").unwrap()), -7);
    }

    #[test]
    fn test_unary_not() {
        assert_eq!(eval_const(parse("~0").unwrap()), -1);
        assert_eq!(eval_const(parse(".not. 0").unwrap()), -1);
    }

    #[test]
    fn test_keyword_ops() {
        assert_eq!(eval_const(parse("8 .mod. 3").unwrap()), 2);
        assert_eq!(eval_const(parse("1 .shl. 4").unwrap()), 16);
        assert_eq!(eval_const(parse("16 .shr. 2").unwrap()), 4);
        assert_eq!(eval_const(parse("5 .and. 3").unwrap()), 1);
        assert_eq!(eval_const(parse("5 .or. 3").unwrap()), 7);
        assert_eq!(eval_const(parse("5 .xor. 3").unwrap()), 6);
    }

    #[test]
    fn test_comparisons() {
        assert_eq!(eval_const(parse("3 .slt. 5").unwrap()), -1);
        assert_eq!(eval_const(parse("5 .slt. 3").unwrap()), 0);
        assert_eq!(eval_const(parse("3 = 3").unwrap()), -1);
        assert_eq!(eval_const(parse("3 <> 4").unwrap()), -1);
        assert_eq!(eval_const(parse("3 != 4").unwrap()), -1);
    }

    #[test]
    fn test_high_low() {
        assert_eq!(eval_const(parse(".high. $1234").unwrap()), 0x12);
        assert_eq!(eval_const(parse(".low. $1234").unwrap()),  0x34);
    }

    #[test]
    fn test_symbol_ref() {
        let rpn = parse("LABEL+4").unwrap();
        let lookup = |name: &[u8]| {
            if name == b"LABEL" { Some(EvalValue { value: 0x1000, section: 1 }) }
            else { None }
        };
        let res = eval_rpn(&rpn, 0, 0, 1, &lookup).unwrap();
        assert_eq!(res, EvalValue { value: 0x1004, section: 1 });
    }

    #[test]
    fn test_left_assoc() {
        // 10 - 3 - 2 = (10-3)-2 = 5 （左結合）
        assert_eq!(eval_const(parse("10-3-2").unwrap()), 5);
    }

    #[test]
    fn test_nested_parens() {
        // ((2+3)*(4-1)) = 15
        assert_eq!(eval_const(parse("((2+3)*(4-1))").unwrap()), 15);
    }

    #[test]
    fn test_defined() {
        let rpn = parse(".defined. MYSYM").unwrap();
        // 未定義の場合は UndefinedSymbol エラーではなく、定義チェック
        // .defined. の特殊シンボル参照が生成されているか確認
        let has_defined = rpn.iter().any(|t| {
            if let RPNToken::SymbolRef(name) = t {
                name.starts_with(b"\x01defined\x01")
            } else { false }
        });
        assert!(has_defined);
    }

    #[test]
    fn test_pos_after_parse() {
        let src = b"42+1,rest";
        let mut pos = 0;
        parse_expr(src, &mut pos).unwrap();
        // ',' で式が終了しているはず
        assert_eq!(pos, 4); // "42+1"
    }
}
