// HAS060のコマンドラインオプション解析・設定およびデフォルト定数を網羅して定義しており、
// 一部オプション（ローカルラベルの最大長制限など）が現在未参照である警告を抑制するために付与しています。
#![allow(dead_code)]
// デフォルト値定数（has.equ / work.s より）
pub const DEFAULT_PRN_WIDTH: u16 = 136;
pub const DEFAULT_PRN_PAGE_LINES: u16 = 58;
pub const DEFAULT_PRN_CODE_WIDTH: u16 = 16;
pub const DEFAULT_LOCAL_LEN_MAX: u16 = 4;
pub const DEFAULT_LOCAL_NUM_MAX: u16 = 10000;
pub const DEFAULT_CPU_NUMBER: u32 = 68000;

/// -b オプション：PC間接→絶対ロング変換モード
#[derive(Debug, Clone, PartialEq)]
pub enum PcToAbslMode {
    Disabled, // 0: 禁止
    M68k,     // 1: 68000コード生成（BRATOJBRA, LONGABS）
    Mem,      // 2: i-cache回避（lea/pea以外）
    M68kMem,  // 3: 1+2
    All,      // 4: デバッグ用（全て）
    M68kAll,  // 5: 1+4
}

/// コマンドラインオプション全体
#[derive(Debug)]
pub struct Options {
    // ---- ファイル ----
    /// ソースファイル名（バイト列）
    pub source_file: Option<Vec<u8>>,
    /// オブジェクトファイル名（None = ソースと同名.o）
    pub object_file: Option<Vec<u8>>,
    /// PRNファイル名（None = ソースと同名.prn）
    pub prn_file: Option<Vec<u8>>,
    /// シンボルファイル名（None = 標準出力）
    pub sym_file: Option<Vec<u8>>,
    /// テンポラリパス（-t）
    pub temp_path: Option<Vec<u8>>,
    /// インクルードパスリスト（-i、複数可）
    pub include_paths_env: Option<Vec<u8>>, // 環境変数で指定
    pub include_paths_cmd: Option<Vec<u8>>, // コマンドラインで指定

    // ---- 出力制御 ----
    /// PRNファイル作成（-p）
    pub make_prn: bool,
    /// シンボルファイル作成（-x）
    pub make_sym: bool,
    /// SCDデバッグ情報出力（-g）
    pub make_sym_deb: bool,
    /// 起動時タイトル表示（-l）
    pub disp_title: bool,
    /// ワーニングレベル（0-4、デフォルト0xFF→2相当）（-w）
    pub warn_level: i8, // -1 = デフォルト(2)

    // ---- 最適化 ----
    /// 前方参照最適化禁止（-n）
    pub no_forward_opt: bool,
    /// -c0 の禁止フラグも含む（-c0,-c4等で変化）
    pub optimize_disabled: bool,
    /// v2互換モード（-c2）
    pub compat_mode: bool,
    pub compat_sw_a: bool, // -a スイッチが指定された（v2互換時）
    pub compat_sw_q: bool, // -q スイッチが指定された（v2互換時）
    /// 絶対ショート変換禁止
    pub no_abs_short: bool,
    /// クイックイミディエイト変換禁止
    pub no_quick: bool,
    /// ゼロディスプレースメント削除禁止
    pub no_null_disp: bool,
    /// 分岐命令削除禁止
    pub no_bra_cut: bool,
    /// 拡張最適化フラグ群（-c4 で全て有効）
    pub opt_clr: bool,
    pub opt_movea: bool,
    pub opt_adda_suba: bool,
    pub opt_cmpa: bool,
    pub opt_lea: bool,
    pub opt_asl: bool,
    pub opt_cmp0: bool,
    pub opt_move0: bool,
    pub opt_cmpi0: bool,
    pub opt_sub_addi0: bool,
    pub opt_bsr: bool,
    pub opt_jmp_jsr: bool,
    /// BRA/BSR/BccをJBRA/JBSR/JBccにする（-b1等）
    pub bra_to_jbra: bool,

    // ---- PC間接/絶対変換 ----
    /// PC間接→絶対ロング変換モード（-b）
    pub pc_to_absl_mode: PcToAbslMode,
    /// 絶対ロングをoptional PC間接にする（-1）
    pub absl_to_opc: bool,

    // ---- 外部参照 ----
    /// 外部参照オフセットデフォルトをロングに（-e）
    pub ext_short: bool,
    pub ext_size_flag: bool,
    /// 未定義シンボルを外部参照に（-u）
    pub all_xref: bool,
    /// 全シンボルを外部定義に（-d）
    pub all_xdef: bool,

    // ---- CPU ----
    /// 初期CPU型情報（-m）
    pub cpu: crate::context::CpuType,

    // ---- シンボル ----
    /// シンボル識別長を8バイトに（-8）
    pub sym_len8: bool,
    /// プレデファインシンボルを定義する（-y1）
    pub predefine: bool,
    /// コマンドラインで定義するシンボル（-s symbol[=n]）
    pub symbol_defs: Vec<(Vec<u8>, i32)>,
    /// シンボル上書き禁止強化（-j bit0: SET, bit1: OFFSYM）
    pub ow_set: bool,
    pub ow_offsym: bool,

    // ---- ローカルラベル ----
    /// 数字ローカルラベルの最大桁数（-s n, 1-4）
    pub local_len_max: u16,
    /// 数字ローカルラベルの最大番号+1
    pub local_num_max: u16,

    // ---- PRNフォーマット ----
    pub prn_no_page_ff: bool,
    pub prn_is_lall: bool,
    pub prn_width: u16,
    pub prn_page_lines: u16,
    pub prn_code_width: u16,

    // ---- ソフトウェアエミュレーション展開 ----
    /// FScc→FBcc展開（-cfscc[=6]）: 0=禁止, 0xFF00=全CPU, C060=68060のみ
    pub expand_fscc: u16,
    /// MOVEP→MOVE展開（-cmovep[=6]）
    pub expand_movep: u16,

    // ---- 68060エラッタ対策 ----
    /// エラッタ対策禁止（-k1）
    pub ignore_errata: bool,
    pub f43g_test: bool,

    // ---- g2asモード ----
    /// 実行ファイル名が 'g2as' で始まる
    pub g2as_mode: bool,

    // ---- 拡張アライン ----
    pub make_align: bool,
    /// オリジナル互換のエラー表示形式を強制するフラグ
    pub compat_error_format: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            source_file: None,
            object_file: None,
            prn_file: None,
            sym_file: None,
            temp_path: None,
            include_paths_env: None,
            include_paths_cmd: None,

            make_prn: false,
            make_sym: false,
            make_sym_deb: false,
            disp_title: false,
            warn_level: -1, // デフォルト: 後で 2 に解決

            no_forward_opt: false,
            optimize_disabled: false,
            compat_mode: false,
            compat_sw_a: false,
            compat_sw_q: false,
            no_abs_short: false,
            no_quick: false,
            no_null_disp: false,
            no_bra_cut: false,
            opt_clr: false,
            opt_movea: false,
            opt_adda_suba: false,
            opt_cmpa: false,
            opt_lea: false,
            opt_asl: false,
            opt_cmp0: false,
            opt_move0: false,
            opt_cmpi0: false,
            opt_sub_addi0: false,
            opt_bsr: false,
            opt_jmp_jsr: false,
            bra_to_jbra: false,

            pc_to_absl_mode: PcToAbslMode::Disabled,
            absl_to_opc: false,

            ext_short: false,
            ext_size_flag: false,
            all_xref: false,
            all_xdef: false,

            cpu: crate::context::CpuType::default_68000(),

            sym_len8: false,
            predefine: false,
            symbol_defs: Vec::new(),
            ow_set: false,
            ow_offsym: false,

            local_len_max: DEFAULT_LOCAL_LEN_MAX,
            local_num_max: DEFAULT_LOCAL_NUM_MAX,

            prn_no_page_ff: false,
            prn_is_lall: false,
            prn_width: DEFAULT_PRN_WIDTH,
            prn_page_lines: DEFAULT_PRN_PAGE_LINES,
            prn_code_width: DEFAULT_PRN_CODE_WIDTH,

            expand_fscc: 0,
            expand_movep: 0,

            ignore_errata: false,
            f43g_test: true,

            g2as_mode: false,
            make_align: false,
            compat_error_format: false,
        }
    }
}

impl Options {
    /// 実効ワーニングレベル（-1 = デフォルト2）
    pub fn effective_warn_level(&self) -> u8 {
        if self.warn_level < 0 {
            2
        } else {
            self.warn_level as u8
        }
    }
}

/// コマンドライン解析エラー
#[derive(Debug)]
pub enum ParseError {
    /// 使用法エラー（usage表示が必要）
    Usage(String),
    /// ソースファイルが複数指定された
    MultipleSourceFiles,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::Usage(msg) => write!(f, "{}", msg),
            ParseError::MultipleSourceFiles => write!(f, "複数のファイル名は指定できません"),
        }
    }
}
