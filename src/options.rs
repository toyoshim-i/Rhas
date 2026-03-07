#![allow(dead_code)]
/// HAS060X.X コマンドラインオプション（完全互換）
///
/// オリジナルと同じ単一文字スイッチ方式を採用する。
/// 複数スイッチの連結（`-c4u` = `-c4 -u`）にも対応する。
use std::ffi::OsStr;
use crate::utils;

// デフォルト値定数（has.equ / work.s より）
pub const DEFAULT_PRN_WIDTH: u16 = 136;
pub const DEFAULT_PRN_PAGE_LINES: u16 = 58;
pub const DEFAULT_PRN_CODE_WIDTH: u16 = 16;
pub const DEFAULT_LOCAL_LEN_MAX: u16 = 4;
pub const DEFAULT_LOCAL_NUM_MAX: u16 = 10000;
pub const DEFAULT_CPU_NUMBER: u32 = 68000;

/// CPUタイプビット（cputype.equ より）
pub mod cpu {
    pub const C000: u16 = 1 << 8;
    pub const C010: u16 = 1 << 9;
    pub const C020: u16 = 1 << 10;
    pub const C030: u16 = 1 << 11;
    pub const C040: u16 = 1 << 12;
    pub const C060: u16 = 1 << 13;
    pub const CMMU: u16 = 1 << 14;
    pub const CFPP: u16 = 1 << 15;
    pub const C520: u16 = 1 << 0;
    pub const C530: u16 = 1 << 1;
    pub const C540: u16 = 1 << 2;
}

/// -b オプション：PC間接→絶対ロング変換モード
#[derive(Debug, Clone, PartialEq)]
pub enum PcToAbslMode {
    Disabled,  // 0: 禁止
    M68k,      // 1: 68000コード生成（BRATOJBRA, LONGABS）
    Mem,       // 2: i-cache回避（lea/pea以外）
    M68kMem,   // 3: 1+2
    All,       // 4: デバッグ用（全て）
    M68kAll,   // 5: 1+4
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
    pub include_paths_env: Option<Vec<u8>>,   // 環境変数で指定
    pub include_paths_cmd: Option<Vec<u8>>,   // コマンドラインで指定

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
    pub warn_level: i8,   // -1 = デフォルト(2)

    // ---- 最適化 ----
    /// 前方参照最適化禁止（-n）
    pub no_forward_opt: bool,
    /// -c0 の禁止フラグも含む（-c0,-c4等で変化）
    pub optimize_disabled: bool,
    /// v2互換モード（-c2）
    pub compat_mode: bool,
    pub compat_sw_a: bool,   // -a スイッチが指定された（v2互換時）
    pub compat_sw_q: bool,   // -q スイッチが指定された（v2互換時）
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
    /// 初期CPUナンバー（-m）
    pub cpu_number: u32,
    /// 初期CPUタイプビット
    pub cpu_type: u16,

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

            cpu_number: DEFAULT_CPU_NUMBER,
            cpu_type: cpu::C000,

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
        }
    }
}

impl Options {
    /// 実効ワーニングレベル（-1 = デフォルト2）
    pub fn effective_warn_level(&self) -> u8 {
        if self.warn_level < 0 { 2 } else { self.warn_level as u8 }
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

/// コマンドライン全体の引数リスト（環境変数+コマンドライン）を解析する
///
/// オリジナルの `docmdline` に相当する。
/// 環境変数 `HAS`（g2asモードなら `G2AS`）の内容をコマンドラインの前に挿入して処理する。
pub fn parse_args<I, S>(args: I, g2as_mode: bool) -> Result<Options, ParseError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut opts = Options { g2as_mode, ..Default::default() };

    // 環境変数 HAS または G2AS を取得
    let env_name = if g2as_mode { "G2AS" } else { "HAS" };
    let env_args = std::env::var(env_name).unwrap_or_default();

    // 全引数を収集（環境変数→コマンドライン順）
    let mut all_args: Vec<Vec<u8>> = Vec::new();

    // 環境変数を空白で分割して追加（原文と同様に単純な空白区切り）
    if !env_args.is_empty() {
        let env_bytes = env_args.as_bytes().to_vec();
        let mut parts = split_args(&env_bytes);
        all_args.append(&mut parts);
    }

    // コマンドライン引数を追加
    for arg in args {
        let s = arg.as_ref().to_string_lossy();
        all_args.push(s.as_bytes().to_vec());
    }

    // 引数を解析
    let mut i = 0;
    // インクルードパス収集用
    let mut inc_path_env: Vec<Vec<u8>> = Vec::new();
    let mut inc_path_cmd: Vec<Vec<u8>> = Vec::new();
    let mut in_env_section = !env_args.is_empty();
    let env_arg_count = if env_args.is_empty() { 0 } else {
        split_args(env_args.as_bytes()).len()
    };

    while i < all_args.len() {
        if i == env_arg_count {
            in_env_section = false;
        }
        let arg = &all_args[i];
        i += 1;

        if arg.is_empty() {
            continue;
        }

        if arg[0] == b'-' {
            // スイッチ解析（複数連結に対応）
            let inc_list = if in_env_section {
                &mut inc_path_env
            } else {
                &mut inc_path_cmd
            };
            let remaining = &all_args[i..];
            let consumed = process_switch(
                &arg[1..],
                remaining,
                &mut opts,
                inc_list,
            )?;
            i += consumed;
        } else {
            // ファイル名
            if opts.source_file.is_some()
                && opts.prn_file.as_deref() != Some(arg.as_slice())
                && opts.sym_file.as_deref() != Some(arg.as_slice())
            {
                return Err(ParseError::MultipleSourceFiles);
            }
            opts.source_file = Some(arg.clone());
        }
    }

    // インクルードパスを文字列として保存（単純化）
    if !inc_path_env.is_empty() {
        opts.include_paths_env = Some(flatten_paths(&inc_path_env));
    }
    if !inc_path_cmd.is_empty() {
        opts.include_paths_cmd = Some(flatten_paths(&inc_path_cmd));
    }

    // ソースファイルがなければ usage
    if opts.source_file.is_none() {
        return Err(ParseError::Usage(String::new()));
    }

    // v2互換モードの後処理（-cと-a/-qの組み合わせ）
    if opts.compat_mode {
        opts.no_abs_short = !opts.compat_sw_a;
        opts.no_quick = opts.compat_sw_q;
    }

    Ok(opts)
}

/// スイッチ文字列（`-` の直後から）を解析する。
/// 返値は追加消費した引数の数。
fn process_switch(
    chars: &[u8],
    remaining_args: &[Vec<u8>],
    opts: &mut Options,
    inc_list: &mut Vec<Vec<u8>>,
) -> Result<usize, ParseError> {
    let mut pos = 0;
    let mut consumed = 0;

    while pos < chars.len() {
        let ch = chars[pos] | 0x20; // 小文字化
        pos += 1;
        match ch {
            b't' => {
                // -t path
                let (s, n) = get_cmd_string(&chars[pos..], remaining_args, consumed)?;
                opts.temp_path = Some(s);
                consumed += n;
                break; // 次の引数へ
            }
            b'o' => {
                // -o name
                let (mut name, n) = get_cmd_string(&chars[pos..], remaining_args, consumed)?;
                consumed += n;
                // 拡張子がなければ .o を付ける
                if !name.contains(&b'.') {
                    name.extend_from_slice(b".o");
                }
                opts.object_file = Some(name);
                break;
            }
            b'i' => {
                // -i path
                let (path, n) = get_cmd_string(&chars[pos..], remaining_args, consumed)?;
                consumed += n;
                inc_list.push(path);
                break;
            }
            b'p' => {
                // -p [file]
                opts.make_prn = true;
                let (file, n) = get_optional_filename(&chars[pos..], remaining_args, consumed);
                consumed += n;
                if let Some(f) = file {
                    opts.prn_file = Some(f);
                }
                break;
            }
            b'x' => {
                // -x [file]
                opts.make_sym = true;
                let (file, n) = get_optional_filename(&chars[pos..], remaining_args, consumed);
                consumed += n;
                if let Some(f) = file {
                    opts.sym_file = Some(f);
                }
                break;
            }
            b'n' => {
                // -n: 最適化省略
                opts.no_forward_opt = true;
                opts.ignore_errata = true;
                opts.f43g_test = false;
            }
            b'w' => {
                // -w [level]
                let (level, n) = get_optional_num(&chars[pos..]);
                pos += n;
                opts.warn_level = match level {
                    Some(v) if v <= 4 => v as i8,
                    Some(_) => return Err(ParseError::Usage("-w: レベルは0-4".into())),
                    None => 2,
                };
            }
            b'u' => opts.all_xref = true,
            b'd' => opts.all_xdef = true,
            b'8' => opts.sym_len8 = true,
            b'l' => opts.disp_title = true,
            b'e' => {
                opts.ext_short = true;
                // 68000/68010以外ではサイズフラグも設定
                // （CPU初期化後に再評価するため、ここでは保留）
                opts.ext_size_flag = true;
            }
            b'g' => opts.make_sym_deb = true,
            b'm' => {
                // -m <cpu>
                let (s, n) = get_cmd_string(&chars[pos..], remaining_args, consumed)?;
                consumed += n;
                let num_str = utils::bytes_to_string(&s);
                let num: u32 = num_str.trim().parse().unwrap_or(0);
                if let Some((cnum, ctype)) = cpu_number_to_type(num) {
                    opts.cpu_number = cnum;
                    opts.cpu_type = ctype;
                } else if num > 1000 && num < 32768 {
                    // 最大シンボル数指定（無視）
                } else {
                    return Err(ParseError::Usage(format!("-m: 不正なCPU指定 '{}'", num_str)));
                }
                break;
            }
            b's' => {
                // -s n（ローカルラベル最大桁数）or -s symbol[=n]（シンボル定義）
                let rest = &chars[pos..];
                if !rest.is_empty() && rest[0].is_ascii_digit() {
                    let d = (rest[0] - b'0') as u16;
                    if d == 0 || d > 4 {
                        return Err(ParseError::Usage("-s: 1-4を指定してください".into()));
                    }
                    pos += 1;
                    opts.local_len_max = d;
                    opts.local_num_max = [10, 100, 1000, 10000][(d - 1) as usize];
                } else {
                    // シンボル定義
                    let (sym_str, n) = get_cmd_string(rest, remaining_args, consumed)?;
                    consumed += n;
                    let (name, val) = parse_symbol_def(&sym_str)?;
                    opts.symbol_defs.push((name, val));
                    break;
                }
            }
            b'f' => {
                // -f [f,m,w,p,c]
                let n = parse_prn_format(&chars[pos..], opts);
                pos += n;
            }
            b'c' => {
                // -c[n|mnemonic]
                let n = parse_c_option(&chars[pos..], opts)?;
                pos += n;
            }
            b'b' => {
                // -b[n]
                let n = parse_b_option(&chars[pos..], opts)?;
                pos += n;
            }
            b'1' => {
                // -1: 絶対ロング → optional PC間接
                opts.absl_to_opc = true;
                opts.ext_short = true;
                opts.ext_size_flag = true;
                // -b1 も設定
                opts.bra_to_jbra = true;
            }
            b'y' => {
                // -y[n]
                let n = parse_optional_01(&chars[pos..]);
                pos += n.0;
                match n.1 {
                    Some(1) | None => opts.predefine = true,
                    Some(0) => opts.predefine = false,
                    _ => return Err(ParseError::Usage("-y: 0または1を指定".into())),
                }
            }
            b'k' => {
                // -k[n]: エラッタ対策
                let n = parse_optional_01(&chars[pos..]);
                pos += n.0;
                match n.1 {
                    Some(1) | None => {
                        opts.ignore_errata = true;
                        opts.f43g_test = false;
                    }
                    Some(0) => {
                        opts.ignore_errata = false;
                        opts.optimize_disabled = false;
                    }
                    _ => return Err(ParseError::Usage("-k: 0または1を指定".into())),
                }
            }
            b'j' => {
                // -j[n]
                let (val, n) = get_optional_num(&chars[pos..]);
                pos += n;
                let v = val.unwrap_or(0xFF) as u8;
                opts.ow_set = (v & 1) != 0;
                opts.ow_offsym = (v & 2) != 0;
            }
            // ダミーオプション（無視）
            b'a' => opts.compat_sw_a = true,
            b'q' => opts.compat_sw_q = true,
            b'z' | b'r' => {}
            _ => {
                return Err(ParseError::Usage(format!("不明なオプション: -{}", ch as char)));
            }
        }
    }

    Ok(consumed)
}

/// `-c` オプションを解析。返値は消費文字数。
fn parse_c_option(chars: &[u8], opts: &mut Options) -> Result<usize, ParseError> {
    if chars.is_empty() {
        // -c のみ → v2互換（-c2 と同じ）
        apply_c2(opts);
        return Ok(0);
    }
    match chars[0] {
        b'0' => { apply_c0(opts); Ok(1) }
        b'1' => { apply_c1(opts); Ok(1) }
        b'2' => { apply_c2(opts); Ok(1) }
        b'3' => { apply_c3(opts); Ok(1) }
        b'4' => { apply_c4(opts); Ok(1) }
        _ => {
            // -c<mnemonic> を解析（fscc / movep / all）
            let end = chars.iter().position(|&c| {
                !c.is_ascii_alphanumeric() && c != b'_'
            }).unwrap_or(chars.len());
            let mnem: Vec<u8> = chars[..end].iter().map(|&c| c | 0x20).collect();
            let mut skip = end;
            // =6 サフィックス
            let cpu_mask = if chars.get(skip) == Some(&b'=') && chars.get(skip + 1) == Some(&b'6') {
                skip += 2;
                cpu::C060
            } else {
                0xFF00 // 全CPU
            };
            match mnem.as_slice() {
                b"fscc" => opts.expand_fscc = cpu_mask,
                b"movep" => opts.expand_movep = cpu_mask,
                b"all" => {
                    opts.expand_fscc = cpu_mask;
                    opts.expand_movep = cpu_mask;
                }
                _ => return Err(ParseError::Usage(format!(
                    "-c: 不明なニーモニック '{}'",
                    utils::bytes_to_string(&mnem)
                ))),
            }
            Ok(skip)
        }
    }
}

fn apply_c0(opts: &mut Options) {
    opts.optimize_disabled = true;
    opts.ignore_errata = true;
    opts.f43g_test = false;
    opts.compat_mode = false;
    opts.no_abs_short = true;
    opts.no_quick = true;
    opts.no_null_disp = true;
    opts.no_bra_cut = true;
    set_ext_opts(opts, false);
}
fn apply_c1(opts: &mut Options) {
    opts.no_null_disp = true;
}
fn apply_c2(opts: &mut Options) {
    opts.compat_mode = true;
    opts.no_null_disp = true;
    opts.no_bra_cut = true;
    set_ext_opts(opts, false);
}
fn apply_c3(opts: &mut Options) {
    apply_c2_off(opts);
    set_ext_opts(opts, false);
}
fn apply_c4(opts: &mut Options) {
    opts.optimize_disabled = false;
    set_ext_opts(opts, true);
    apply_c2_off(opts);
}
fn apply_c2_off(opts: &mut Options) {
    opts.compat_mode = false;
    opts.no_abs_short = false;
    opts.no_quick = false;
    opts.no_null_disp = false;
    opts.no_bra_cut = false;
}
fn set_ext_opts(opts: &mut Options, v: bool) {
    opts.opt_clr = v;
    opts.opt_movea = v;
    opts.opt_adda_suba = v;
    opts.opt_cmpa = v;
    opts.opt_lea = v;
    opts.opt_asl = v;
    opts.opt_cmp0 = v;
    opts.opt_move0 = v;
    opts.opt_cmpi0 = v;
    opts.opt_sub_addi0 = v;
    opts.opt_bsr = v;
    opts.opt_jmp_jsr = v;
}

/// `-b` オプションを解析。返値は消費文字数。
fn parse_b_option(chars: &[u8], opts: &mut Options) -> Result<usize, ParseError> {
    let (level, n) = get_optional_num(chars);
    let level = level.unwrap_or(1);
    match level {
        0 => {
            opts.pc_to_absl_mode = PcToAbslMode::Disabled;
            opts.bra_to_jbra = false;
        }
        1 => {
            opts.pc_to_absl_mode = PcToAbslMode::M68k;
            opts.bra_to_jbra = true;
        }
        2 => {
            opts.pc_to_absl_mode = PcToAbslMode::Mem;
            opts.bra_to_jbra = false;
        }
        3 => {
            opts.pc_to_absl_mode = PcToAbslMode::M68kMem;
            opts.bra_to_jbra = true;
        }
        4 => {
            opts.pc_to_absl_mode = PcToAbslMode::All;
            opts.bra_to_jbra = false;
        }
        5 => {
            opts.pc_to_absl_mode = PcToAbslMode::M68kAll;
            opts.bra_to_jbra = true;
        }
        _ => return Err(ParseError::Usage("-b: 0-5を指定してください".into())),
    }
    Ok(n)
}

/// `-f` オプション（PRNフォーマット）を解析。返値は消費文字数。
fn parse_prn_format(chars: &[u8], opts: &mut Options) -> usize {
    let mut pos = 0;
    // f（ページング）
    let (v, n) = get_optional_num(&chars[pos..]);
    pos += n;
    match v {
        None => { opts.prn_no_page_ff = true; return pos; }
        Some(0) => opts.prn_no_page_ff = true,
        Some(1) => opts.prn_no_page_ff = false,
        _ => { opts.prn_no_page_ff = true; return pos; }
    }
    if chars.get(pos) != Some(&b',') { return pos; }
    pos += 1;
    // m（マクロ展開）
    let (v, n) = get_optional_num(&chars[pos..]);
    pos += n;
    if let Some(v) = v {
        opts.prn_is_lall = v != 0;
    }
    if chars.get(pos) != Some(&b',') { return pos; }
    pos += 1;
    // w（幅）
    let (v, n) = get_optional_num(&chars[pos..]);
    pos += n;
    if let Some(v) = v {
        if (80..256).contains(&v) {
            opts.prn_width = (v & !7) as u16;
        }
    }
    if chars.get(pos) != Some(&b',') { return pos; }
    pos += 1;
    // p（ページ行数）
    let (v, n) = get_optional_num(&chars[pos..]);
    pos += n;
    if let Some(v) = v {
        if (10..256).contains(&v) {
            opts.prn_page_lines = v as u16;
        }
    }
    if chars.get(pos) != Some(&b',') { return pos; }
    pos += 1;
    // c（コード幅）
    let (v, n) = get_optional_num(&chars[pos..]);
    pos += n;
    if let Some(v) = v {
        if (4..65).contains(&v) {
            opts.prn_code_width = (v & !3) as u16;
        }
    }
    pos
}

/// 0または1のオプション数値を取得。返値は（値, 消費文字数）。
fn parse_optional_01(chars: &[u8]) -> (usize, Option<u32>) {
    if chars.first().map(|c| c.is_ascii_digit()).unwrap_or(false) {
        let v = (chars[0] - b'0') as u32;
        (1, Some(v))
    } else {
        (0, None)
    }
}

/// オプション引数の数値を取得（10進、連続する数字のみ）。
/// 返値は（値, 消費文字数）。数字がなければ None。
fn get_optional_num(chars: &[u8]) -> (Option<u32>, usize) {
    if chars.is_empty() || !chars[0].is_ascii_digit() {
        return (None, 0);
    }
    let mut val: u32 = 0;
    let mut n = 0;
    while n < chars.len() && chars[n].is_ascii_digit() {
        val = val * 10 + (chars[n] - b'0') as u32;
        n += 1;
    }
    (Some(val), n)
}

/// スイッチの引数文字列を取得（残り文字または次の引数）。
/// 返値は（文字列, 消費追加引数数）。
fn get_cmd_string(
    chars: &[u8],
    remaining: &[Vec<u8>],
    already_consumed: usize,
) -> Result<(Vec<u8>, usize), ParseError> {
    if !chars.is_empty() {
        // スイッチに続く文字列（例: -tpath）
        return Ok((chars.to_vec(), 0));
    }
    // 次の引数
    let next_idx = already_consumed;
    if next_idx < remaining.len() {
        let next = &remaining[next_idx];
        if !next.is_empty() {
            return Ok((next.clone(), 1));
        }
    }
    Err(ParseError::Usage("引数が不足しています".into()))
}

/// オプショナルなファイル名を取得（次の引数が `-` 以外ならファイル名）。
fn get_optional_filename(
    chars: &[u8],
    remaining: &[Vec<u8>],
    already_consumed: usize,
) -> (Option<Vec<u8>>, usize) {
    if !chars.is_empty() {
        return (Some(chars.to_vec()), 0);
    }
    let next_idx = already_consumed;
    if let Some(next) = remaining.get(next_idx) {
        if !next.is_empty() && next[0] != b'-' {
            return (Some(next.clone()), 1);
        }
    }
    (None, 0)
}

/// `-s symbol[=n]` のシンボル定義を解析する。
fn parse_symbol_def(s: &[u8]) -> Result<(Vec<u8>, i32), ParseError> {
    if let Some(eq_pos) = s.iter().position(|&c| c == b'=') {
        let name = s[..eq_pos].to_vec();
        let val_str = &s[eq_pos + 1..];
        let negative = val_str.first() == Some(&b'-');
        let digits = if negative { &val_str[1..] } else { val_str };
        let val: i32 = std::str::from_utf8(digits)
            .ok()
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| ParseError::Usage("-s: 数値が不正".into()))?;
        Ok((name, if negative { -val } else { val }))
    } else {
        Ok((s.to_vec(), 0))
    }
}

/// 空白区切りで引数を分割する（簡易版、クォートは非対応）
fn split_args(s: &[u8]) -> Vec<Vec<u8>> {
    let mut result = Vec::new();
    let mut current = Vec::new();
    for &b in s {
        if b == b' ' || b == b'\t' {
            if !current.is_empty() {
                result.push(current.clone());
                current.clear();
            }
        } else {
            current.push(b);
        }
    }
    if !current.is_empty() {
        result.push(current);
    }
    result
}

/// パスリストを NUL 区切りのバイト列に変換（内部管理用）
fn flatten_paths(paths: &[Vec<u8>]) -> Vec<u8> {
    let mut result = Vec::new();
    for p in paths {
        result.extend_from_slice(p);
        result.push(0);
    }
    result
}

/// CPU番号をCPUタイプビットに変換する。
/// 返値: Some（CPUナンバー, CPUタイプ）または None（不正な値）
pub fn cpu_number_to_type(n: u32) -> Option<(u32, u16)> {
    match n {
        68000 => Some((68000, cpu::C000)),
        68010 => Some((68010, cpu::C010)),
        68020 => Some((68020, cpu::C020)),
        68030 => Some((68030, cpu::C030)),
        68040 => Some((68040, cpu::C040)),
        68060 => Some((68060, cpu::C060)),
        5200 => Some((5200, cpu::C520)),
        5300 => Some((5300, cpu::C530)),
        5400 => Some((5400, cpu::C540)),
        _ => None,
    }
}

/// バージョン情報
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
        assert_eq!(opts.cpu_number, 68000);
        assert_eq!(opts.cpu_type, cpu::C000);
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
        assert_eq!(opts.cpu_number, 68020);
        assert_eq!(opts.cpu_type, cpu::C020);
    }

    #[test]
    fn test_w_option() {
        let result = parse_args(["-w0", "foo.s"], false);
        let opts = result.unwrap();
        assert_eq!(opts.effective_warn_level(), 0);
    }
}
