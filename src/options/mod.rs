pub mod cpu;
mod types;
mod parser;

pub use types::{
    Options, ParseError, PcToAbslMode,
    DEFAULT_PRN_WIDTH, DEFAULT_PRN_PAGE_LINES, DEFAULT_PRN_CODE_WIDTH,
    DEFAULT_LOCAL_LEN_MAX, DEFAULT_LOCAL_NUM_MAX, DEFAULT_CPU_NUMBER,
};
pub use parser::parse_args;
pub use cpu::cpu_number_to_type;

pub const VERSION: &str = "1.2.5";
pub const VERSION_BASE: &str = "3.09+91";
pub const COPYRIGHT: &str = "(C) 1990-1994/1996-2023 Y.Nakamura/M.Kamada";
pub const COPYRIGHT_X: &str = "(C) 2026 TcbnErik / Rust port by rhas contributors";

/// タイトルメッセージ
pub fn title_message() -> String {
    format!(
        "HAS060X.X {} {}\n  based on X68k High-speed Assembler v{} {}\n",
        VERSION, COPYRIGHT_X, VERSION_BASE, COPYRIGHT
    )
}

/// 使用法メッセージ
pub fn usage_message() -> String {
    format!(
        "{}使用法: rhas [スイッチ] ファイル名\n\
        \t-1\t\t絶対ロング→PC間接(-b1と-eを伴う)\n\
        \t-8\t\tシンボルの識別長を8バイトにする\n\
        \t-b[n]\t\tPC間接→絶対ロング(0=[禁止],[1]=68000,2=MEM,3=1+2,4=ALL,5=1+4)\n\
        \t-c[n]\t\t最適化(0=禁止(-k1を伴う),1=(d,An)を禁止,[2]=v2互換,3=[v3互換],4=許可)\n\
        \t-c<mnemonic>\tsoftware emulationの命令を展開する(FScc/MOVEP)\n\
        \t-d\t\tすべてのシンボルを外部定義にする\n\
        \t-e\t\t外部参照オフセットのデフォルトをロングワードにする\n\
        \t-f[f,m,w,p,c]\tリストファイルのフォーマット\n\
        \t-g\t\tSCD用デバッグ情報の出力\n\
        \t-i <path>\tインクルードパス指定\n\
        \t-j[n]\t\tシンボルの上書き禁止条件の強化(bit0:[1]=SET,bit1:[1]=OFFSYM)\n\
        \t-k[n]\t\t68060のエラッタ対策(0=[する](-nは無効),[1]=しない)\n\
        \t-l\t\t起動時にタイトルを表示する\n\
        \t-m <680x0|5x00>\tアセンブル対象CPUの指定([68000]〜68060/5200〜5400)\n\
        \t-n\t\tパス1で確定できないサイズの最適化を省略する(-k1を伴う)\n\
        \t-o <name>\tオブジェクトファイル名\n\
        \t-p [file]\tリストファイル作成\n\
        \t-s <n>\t\t数字ローカルラベルの最大桁数の指定(1〜[4])\n\
        \t-s <symbol>[=n]\tシンボルの定義\n\
        \t-t <path>\tテンポラリパス指定\n\
        \t-u\t\t未定義シンボルを外部参照にする\n\
        \t-w[n]\t\tワーニングレベルの指定(0=全抑制,1,[2],3,4=[全通知])\n\
        \t-x [file]\tシンボルの出力\n\
        \t-y[n]\t\tプレデファインシンボル(0=[禁止],[1]=許可)\n\
        \t環境変数 HAS の内容がコマンドラインの手前(-iは後ろ)に挿入されます\n",
        title_message()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_defaults() {
        let opts = Options::default();
        assert_eq!(opts.cpu.number, 68000);
        assert_eq!(opts.cpu.features, cpu::C000);
        assert_eq!(opts.local_len_max, 4);
        assert_eq!(opts.local_num_max, 10000);
    }

    #[test]
    fn test_basic_parse() {
        let result = parse_args(["source.s"], false);
        let opts = result.unwrap();
        assert_eq!(opts.source_file, Some(b"source.s".to_vec()));
        assert!(!opts.all_xref);
    }

    #[test]
    fn test_parse_cu() {
        // -c4 -u
        let result = parse_args(["-c4", "-u", "source.s"], false);
        let opts = result.unwrap();
        assert!(opts.opt_clr);
        assert!(opts.all_xref);
    }

    #[test]
    fn test_no_source() {
        let result = parse_args::<[&str; 0], &str>([], false);
        assert!(matches!(result, Err(ParseError::Usage(_))));
    }

    #[test]
    fn test_c_option() {
        let result = parse_args(["-c4", "foo.s"], false);
        let opts = result.unwrap();
        assert!(opts.opt_clr);
        assert!(!opts.compat_mode);
        assert!(!opts.no_abs_short);
    }

    #[test]
    fn test_m_option() {
        let result = parse_args(["-m68020", "foo.s"], false);
        let opts = result.unwrap();
        assert_eq!(opts.cpu.number, 68020);
        assert_eq!(opts.cpu.features, cpu::C020);
    }

    #[test]
    fn test_w_option() {
        let result = parse_args(["-w0", "foo.s"], false);
        let opts = result.unwrap();
        assert_eq!(opts.effective_warn_level(), 0);
    }
}
