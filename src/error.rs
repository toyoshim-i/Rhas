// 原典（error.s）由来のエラーコード・ワーニングコードテーブルを
// 先行移植しているため、未接続のコードが残っている。
#![allow(dead_code)]
//! rhas エラー・ワーニング処理
//!
//! オリジナルの `error.s` に相当する。
//! エラーコード・ワーニングコードは `errtbl` / `warntbl` マクロ定義から移植。

use std::io::Write;
use crate::utils;

// ----------------------------------------------------------------
// エラーコード
// ----------------------------------------------------------------

/// アセンブラエラーコード（error.s の errtbl マクロに対応）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum ErrorCode {
    // .fail によるエラー
    Forced = 0,
    // シンボル関連
    Redef,
    RedefPredefine,
    RedefSet,
    RedefOffsym,
    // 命令解釈
    BadOpe,
    BadOpeLocal,
    BadOpeLocalLen,
    // シンボル種別不正
    IlSymValue,
    IlSymLocal,
    IlSymReal,
    IlSymRegsym,
    IlSymRegister,
    IlSymPredefXdef,
    IlSymPredefXref,
    IlSymPredefGlobl,
    IlSymLookfor,
    // 式解析
    Expr,
    ExprEa,
    ExprCannotScale,
    ExprScaleFactor,
    ExprFullFormat,
    ExprImmediate,
    // レジスタ
    Reg,
    RegOpc,
    // アドレッシング
    IlAdr,
    // サイズ
    IlSizeOp,
    IlSizePseudo,
    IlSizeMoveUsp,
    IlSizeCfAcc,
    IlSizeFpn,
    IlSizeFprn,
    IlSizeFpcr,
    IlSizeFmovemFpn,
    IlSizeFmovemFpcr,
    IlSizeCfLong,
    IlSizeCfBccL,
    IlSize,
    IlSizePseudoNo,
    IlSizeOpNo,
    IlSizeCcr,
    IlSizeSr,
    IlSizeAn,
    IlSizeMoveToSr,
    IlSizeMoveFrSr,
    IlSize000Long,
    IlSizeSftRotMem,
    IlSizeBitMem,
    IlSizeBitReg,
    IlSize000BccL,
    IlSizeTrapcc,
    // オペランド
    IlOpr,
    IlOprNotFixed,
    IlOprTooMany,
    IlOprPseudoMany,
    IlOprLocal,
    IlOprLocalLen,
    IlOprDsNegative,
    IlOprEndXref,
    IlOprInternalFp,
    // 未定義シンボル
    UndefSym,
    UndefSymLocal,
    UndefSymOffsym,
    // 演算エラー
    DivZero,
    IlRelOutside,
    IlRelConst,
    Overflow,
    IlValue,
    IlQuickAddSubQ,
    IlQuickMoveQ,
    IlQuickMov3Q,
    IlQuickSftRot,
    IlSft,
    // CPU/機能
    FeatureCpu,
    FeatureXref,
    // マクロ・疑似命令
    NoSymMacro,
    NoSymPseudo,
    TooIncld,
    NoFile,
    MisMacExitm,
    MisMacEndm,
    MisMacLocal,
    MisMacSizem,
    MisMacEof,
    TooManyLocSym,
    MacNest,
    MisIfElse,
    MisIfElseif,
    MisIfEndif,
    MisIfElseElseif,
    MisIfEof,
    // 文字列終端
    TermDoubleQuote,
    TermSingleQuote,
    TermBracket,
    // その他
    IlInt,
    OffsymAlign,
}

impl ErrorCode {
    /// エラーメッセージ文字列（error.s の errtbl 文字列から移植）
    pub fn message(self) -> &'static str {
        match self {
            ErrorCode::Forced => ".fail によるエラー",
            ErrorCode::Redef => "シンボル %s は既に定義されています",
            ErrorCode::RedefPredefine => "プレデファインシンボル %s を再定義しようとしました",
            ErrorCode::RedefSet => "シンボル %s は .set(=) 以外で定義されています",
            ErrorCode::RedefOffsym => "シンボル %s は .offsym 以外で定義されています",
            ErrorCode::BadOpe => "命令が解釈できません",
            ErrorCode::BadOpeLocal => "ローカルラベルの記述が不正です",
            ErrorCode::BadOpeLocalLen => "ローカルラベルの桁数が多すぎるため定義できません",
            ErrorCode::IlSymValue => "シンボルの種類が異なります",
            ErrorCode::IlSymLocal => "ローカルシンボルの種類が異なります",
            ErrorCode::IlSymReal => "浮動小数点シンボルの種類が異なります",
            ErrorCode::IlSymRegsym => "シンボル %s は種類が異なるので使えません",
            ErrorCode::IlSymRegister => "シンボル %s はレジスタ名なので使えません",
            ErrorCode::IlSymPredefXdef => "プレデファインシンボル %s は外部定義宣言できません",
            ErrorCode::IlSymPredefXref => "プレデファインシンボル %s は外部参照宣言できません",
            ErrorCode::IlSymPredefGlobl => "プレデファインシンボル %s はグローバル宣言できません",
            ErrorCode::IlSymLookfor => "シンボル %s の定義が参照方法と矛盾しています",
            ErrorCode::Expr => "記述が間違っています",
            ErrorCode::ExprEa => "実効アドレスが解釈できません",
            ErrorCode::ExprCannotScale => "スケールファクタは指定できません",
            ErrorCode::ExprScaleFactor => "スケールファクタの指定が間違っています",
            ErrorCode::ExprFullFormat => "フルフォーマットのアドレッシングは使えません",
            ErrorCode::ExprImmediate => "イミディエイトデータが解釈できません",
            ErrorCode::Reg => "指定できないレジスタです",
            ErrorCode::RegOpc => "このアドレッシングでは opc は使えません",
            ErrorCode::IlAdr => "指定できないアドレッシングです",
            ErrorCode::IlSizeOp => "命令 %s には指定できないサイズです",
            ErrorCode::IlSizePseudo => "疑似命令 %s には指定できないサイズです",
            ErrorCode::IlSizeMoveUsp => "MOVE USP はロングワードサイズのみ指定可能です",
            ErrorCode::IlSizeCfAcc => "MOVE ACC はロングワードサイズのみ指定可能です",
            ErrorCode::IlSizeFpn => "浮動小数点レジスタ直接アドレッシングは拡張サイズのみ指定可能です",
            ErrorCode::IlSizeFprn => "汎用レジスタ直接アドレッシングはロングワードサイズのみ指定可能です",
            ErrorCode::IlSizeFpcr => "FPCR/FPIAR/FPSR はロングワードサイズのみ指定可能です",
            ErrorCode::IlSizeFmovemFpn => "FMOVEM FPn は拡張サイズのみ指定可能です",
            ErrorCode::IlSizeFmovemFpcr => "FMOVEM FPcr はロングワードサイズのみ指定可能です",
            ErrorCode::IlSizeCfLong => "5200/5300 ではロングワードサイズのみ指定可能です",
            ErrorCode::IlSizeCfBccL => "5200/5300 ではロングワードサイズの相対分岐はできません",
            ErrorCode::IlSize => "指定できないサイズです",
            ErrorCode::IlSizePseudoNo => "疑似命令にはサイズを指定できません",
            ErrorCode::IlSizeOpNo => "%s にはサイズを指定できません",
            ErrorCode::IlSizeCcr => "%s to CCR はバイトサイズのみ指定可能です",
            ErrorCode::IlSizeSr => "%s to SR はワードサイズのみ指定可能です",
            ErrorCode::IlSizeAn => "アドレスレジスタはバイトサイズでアクセスできません",
            ErrorCode::IlSizeMoveToSr => "MOVE to CCR/SR はワードサイズのみ指定可能です",
            ErrorCode::IlSizeMoveFrSr => "MOVE from CCR/SR はワードサイズのみ指定可能です",
            ErrorCode::IlSize000Long => "68000/68010 ではロングワードサイズは指定できません",
            ErrorCode::IlSizeSftRotMem => "メモリに対するシフト・ローテートはワードサイズのみ指定可能です",
            ErrorCode::IlSizeBitMem => "メモリに対するビット操作はバイトサイズのみ指定可能です",
            ErrorCode::IlSizeBitReg => "データレジスタに対するビット操作はロングワードサイズのみ指定可能です",
            ErrorCode::IlSize000BccL => "68000/68010 ではロングワードサイズの相対分岐はできません",
            ErrorCode::IlSizeTrapcc => "オペランドのない TRAPcc にはサイズを指定できません",
            ErrorCode::IlOpr => "不正なオペランドです",
            ErrorCode::IlOprNotFixed => "引数が確定していません",
            ErrorCode::IlOprTooMany => "%s のオペランドが多すぎます",
            ErrorCode::IlOprPseudoMany => "%s の引数が多すぎます",
            ErrorCode::IlOprLocal => "ローカルラベルの参照が不正です",
            ErrorCode::IlOprLocalLen => "ローカルラベルの桁数が多すぎるため参照できません",
            ErrorCode::IlOprDsNegative => "%s の引数が負数です",
            ErrorCode::IlOprEndXref => ".end に外部参照値は指定できません",
            ErrorCode::IlOprInternalFp => "浮動小数点数の内部表現の長さが合いません",
            ErrorCode::UndefSym => "シンボル %s が未定義です",
            ErrorCode::UndefSymLocal => "ローカルラベルが未定義です",
            ErrorCode::UndefSymOffsym => "値を確定できません",
            ErrorCode::DivZero => "0 で除算しました",
            ErrorCode::IlRelOutside => "オフセットが範囲外です",
            ErrorCode::IlRelConst => "定数ではなくアドレス値が必要です",
            ErrorCode::Overflow => "オーバーフローしました",
            ErrorCode::IlValue => "不正な値です",
            ErrorCode::IlQuickAddSubQ => "データが 1～8 の範囲外です",
            ErrorCode::IlQuickMoveQ => "データが -128～127 の範囲外です",
            ErrorCode::IlQuickMov3Q => "データが -1,1～7 の範囲外です",
            ErrorCode::IlQuickSftRot => "シフト・ローテートのカウントが範囲外です",
            ErrorCode::IlSft => "シフト・ローテートのカウントが 1～8 の範囲外です",
            ErrorCode::FeatureCpu => "未対応の cpu です",
            ErrorCode::FeatureXref => "外部参照値の埋め込みはできません",
            ErrorCode::NoSymMacro => "マクロ名がありません",
            ErrorCode::NoSymPseudo => "%s で定義するシンボルがありません",
            ErrorCode::TooIncld => ".include のネストが深すぎます",
            ErrorCode::NoFile => "%s するファイルが見つかりません",
            ErrorCode::MisMacExitm => "マクロ展開中ではないのに %s があります",
            ErrorCode::MisMacEndm => ".endm に対応する .macro がありません",
            ErrorCode::MisMacLocal => ".local に対応する .macro がありません",
            ErrorCode::MisMacSizem => "マクロ定義中ではないのに %s があります",
            ErrorCode::MisMacEof => ".endm が足りません",
            ErrorCode::TooManyLocSym => "1 つのマクロの中のローカルシンボルが多すぎます",
            ErrorCode::MacNest => "マクロのネストが深すぎます",
            ErrorCode::MisIfElse => ".else に対応する .if がありません",
            ErrorCode::MisIfElseif => ".elseif に対応する .if がありません",
            ErrorCode::MisIfEndif => "%s に対応する .if がありません",
            ErrorCode::MisIfElseElseif => ".else の後に .elseif があります",
            ErrorCode::MisIfEof => ".endif が足りません",
            ErrorCode::TermDoubleQuote => "\"～\"が閉じていません",
            ErrorCode::TermSingleQuote => "'～'が閉じていません",
            ErrorCode::TermBracket => "<～>が閉じていません",
            ErrorCode::IlInt => "整数の記述が間違っています",
            ErrorCode::OffsymAlign => ".offsym 中に %s は指定できません",
        }
    }
}

// ----------------------------------------------------------------
// ワーニングコード
// ----------------------------------------------------------------

/// ワーニングコード（error.s の warntbl マクロに対応）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WarnCode(pub u8);

pub mod warn {
    use super::WarnCode;
    pub const ABS: WarnCode = WarnCode(0);          // 絶対アドレッシング
    pub const ABS_SHORT: WarnCode = WarnCode(1);    // 絶対ショートアドレッシング
    pub const SHORT: WarnCode = WarnCode(2);        // アドレスレジスタを .w で更新
    pub const SHORT_CMPA: WarnCode = WarnCode(3);   // アドレスレジスタを .w で比較
    pub const SH_VAL_ABSW: WarnCode = WarnCode(4);  // 絶対ショートアドレスが範囲外
    pub const SH_VAL_D16: WarnCode = WarnCode(5);   // オフセットが -32768～32767 の範囲外
    pub const SH_VAL_D8: WarnCode = WarnCode(6);    // オフセットが -128～127 の範囲外
    pub const REGL: WarnCode = WarnCode(7);         // レジスタリストの表記が不正
    pub const ALIGN: WarnCode = WarnCode(8);        // データのアラインメントが不正
    pub const ALIGN_OP: WarnCode = WarnCode(9);     // 命令が奇数アドレスに配置
    pub const SOFT: WarnCode = WarnCode(10);        // ソフトウェアエミュレーションで実行
    pub const F43G: WarnCode = WarnCode(11);        // 浮動小数点命令の直後に NOP を挿入（エラッタ対策）
    pub const MOVETOUSP: WarnCode = WarnCode(12);   // MOVE to USP の直前に MOVEA 挿入（エラッタ対策）
    pub const INSIG_BIT: WarnCode = WarnCode(13);   // CCR/SR の未定義ビットを操作
    pub const INDEX_SZ: WarnCode = WarnCode(14);    // インデックスのサイズが指定されていない
    pub const REDEF_SET: WarnCode = WarnCode(15);   // .set で上書き
    pub const REDEF_OFFSYM: WarnCode = WarnCode(16);// .offsym で上書き
    pub const DS_NEGATIVE: WarnCode = WarnCode(17); // ds の引数が負数
    pub const INTERNAL_FP: WarnCode = WarnCode(18); // 整数を単精度FPの内部表現と見なす
}

/// ワーニングのデフォルト通知レベル（warntbl の lvl フィールド）
pub fn warn_default_level(code: WarnCode) -> u8 {
    match code.0 {
        0..=1 => 4,  // ABS, ABS_SHORT: レベル4以上で通知
        2..=6 => 3,  // SHORT等: レベル3以上
        7..=18 => 1, // その他: レベル1以上
        _ => 1,
    }
}

/// ワーニングメッセージ文字列（%s は後で展開）
pub fn warn_message(code: WarnCode) -> &'static str {
    match code.0 {
        0 => "絶対アドレッシングです",
        1 => "絶対ショートアドレッシングです",
        2 => "アドレスレジスタを %s.w で更新しています",
        3 => "アドレスレジスタを %s.w で比較しています",
        4 => "絶対ショートアドレスが -$8000～$7FFF の範囲外です",
        5 => "オフセットが -32768～32767 の範囲外です",
        6 => "オフセットが -128～127 の範囲外です",
        7 => "レジスタリストの表記が不正です",
        8 => "%s のデータのアラインメントが不正です",
        9 => "命令が奇数アドレスに配置されました",
        10 => "%s はソフトウェアエミュレーションで実行されます",
        11 => "浮動小数点命令の直後に NOP を挿入しました (エラッタ対策)",
        12 => "MOVE to USP の直前に MOVEA.L A0,A0 を挿入しました (エラッタ対策)",
        13 => "CCR/SR の未定義のビットを操作しています",
        14 => "インデックスのサイズが指定されていません",
        15 => "シンボル %s を .set(=) で上書きしました",
        16 => "シンボル %s を .offsym で上書きしました",
        17 => "%s の引数が負数です",
        18 => "整数を単精度浮動小数点数の内部表現と見なします",
        _ => "不明なワーニング",
    }
}

// ----------------------------------------------------------------
// 位置情報
// ----------------------------------------------------------------

/// ソースコード上の位置（ファイル名+行番号）
#[derive(Debug, Clone)]
pub struct SourcePos {
    /// ファイル名（バイト列、最大16文字表示）
    pub filename: Vec<u8>,
    /// 行番号（1始まり）
    pub line: u32,
}

impl SourcePos {
    pub fn new(filename: Vec<u8>, line: u32) -> Self {
        SourcePos { filename, line }
    }

    /// ファイル名を表示用文字列として取得（最大16文字）
    fn filename_display(&self) -> String {
        let s = utils::bytes_to_string(&self.filename);
        if s.len() <= 16 {
            format!("{:<16}", s)
        } else {
            s[..16].to_string()
        }
    }
}

// ----------------------------------------------------------------
// エラー出力
// ----------------------------------------------------------------

/// アセンブラのエラー出力（error.s の printerr に相当）
///
/// フォーマット: `<filename>  <linenum>: Error: <message>\n`
pub fn print_error(
    out: &mut dyn Write,
    pos: &SourcePos,
    code: ErrorCode,
    sym: Option<&[u8]>,
) {
    let msg = format_message(code.message(), sym);
    let _ = writeln!(
        out,
        "{} {:6}: Error: {}",
        pos.filename_display(),
        pos.line,
        msg
    );
}

/// ワーニング出力
pub fn print_warning(
    out: &mut dyn Write,
    pos: &SourcePos,
    code: WarnCode,
    sym: Option<&[u8]>,
    warn_level: u8,
) {
    if warn_level < warn_default_level(code) {
        return;
    }
    let msg = format_message(warn_message(code), sym);
    let _ = writeln!(
        out,
        "{} {:6}: Warning: {}",
        pos.filename_display(),
        pos.line,
        msg
    );
}

/// `%s` を sym で置換する
fn format_message(template: &str, sym: Option<&[u8]>) -> String {
    if let Some(s) = sym {
        let sym_str = utils::bytes_to_string(s);
        template.replacen("%s", &sym_str, 1)
    } else {
        template.to_string()
    }
}

// ----------------------------------------------------------------
// ファイルI/Oエラー
// ----------------------------------------------------------------

/// ファイル操作エラー（アセンブル中のI/Oエラー）
#[derive(Debug)]
pub struct FileError {
    pub path: Vec<u8>,
    pub kind: FileErrorKind,
}

#[derive(Debug)]
pub enum FileErrorKind {
    NotFound,
    AccessDenied,
    ReadError(std::io::Error),
    WriteError(std::io::Error),
    CreateError(std::io::Error),
}

impl std::fmt::Display for FileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let path = utils::bytes_to_string(&self.path);
        match &self.kind {
            FileErrorKind::NotFound => write!(f, "ファイルが見つかりません: {}", path),
            FileErrorKind::AccessDenied => write!(f, "アクセス拒否: {}", path),
            FileErrorKind::ReadError(e) => write!(f, "読み込みエラー: {} ({})", path, e),
            FileErrorKind::WriteError(e) => write!(f, "書き込みエラー: {} ({})", path, e),
            FileErrorKind::CreateError(e) => write!(f, "作成エラー: {} ({})", path, e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_message() {
        assert_eq!(ErrorCode::BadOpe.message(), "命令が解釈できません");
        assert_eq!(ErrorCode::UndefSym.message(), "シンボル %s が未定義です");
    }

    #[test]
    fn test_format_message() {
        let result = format_message("シンボル %s が未定義です", Some(b"LABEL"));
        assert_eq!(result, "シンボル LABEL が未定義です");
    }

    #[test]
    fn test_format_message_no_sym() {
        let result = format_message("記述が間違っています", None);
        assert_eq!(result, "記述が間違っています");
    }

    #[test]
    fn test_warn_level() {
        assert_eq!(warn_default_level(warn::ABS), 4);
        assert_eq!(warn_default_level(warn::REGL), 1);
    }
}
