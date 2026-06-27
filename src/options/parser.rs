use super::cpu::{self, cpu_number_to_type};
use super::types::{Options, ParseError, PcToAbslMode};
use crate::utils;
use std::ffi::OsStr;

/// コマンドライン全体の引数リスト（環境変数+コマンドライン）を解析する
///
/// オリジナルの `docmdline` に相当する。
/// 環境変数 `HAS`（g2asモードなら `G2AS`）の内容をコマンドラインの前に挿入して処理する。
pub fn parse_args<I, S>(args: I, g2as_mode: bool) -> Result<Options, ParseError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut opts = Options {
        g2as_mode,
        ..Default::default()
    };

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
    let env_arg_count = if env_args.is_empty() {
        0
    } else {
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
            let consumed = process_switch(&arg[1..], remaining, &mut opts, inc_list)?;
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
                let n = parse_t_option(chars, remaining_args, consumed, pos, opts)?;
                consumed += n;
                break; // 次の引数へ
            }
            b'o' => {
                let n = parse_o_option(chars, remaining_args, consumed, pos, opts)?;
                consumed += n;
                break;
            }
            b'i' => {
                let n = parse_i_option(chars, remaining_args, consumed, pos, inc_list)?;
                consumed += n;
                break;
            }
            b'p' => {
                let n = parse_p_option(chars, remaining_args, consumed, pos, opts);
                consumed += n;
                break;
            }
            b'x' => {
                let n = parse_x_option(chars, remaining_args, consumed, pos, opts);
                consumed += n;
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
                let n = parse_w_option(&chars[pos..], opts)?;
                pos += n;
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
                let n = parse_m_option(chars, remaining_args, consumed, pos, opts)?;
                consumed += n;
                break;
            }
            b's' => {
                // -s n（ローカルラベル最大桁数）or -s symbol[=n]（シンボル定義）
                let (pos_add, cons_add, should_break) =
                    parse_s_option(chars, remaining_args, consumed, pos, opts)?;
                pos += pos_add;
                consumed += cons_add;
                if should_break {
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
                let n = parse_y_option(&chars[pos..], opts)?;
                pos += n;
            }
            b'k' => {
                // -k[n]: エラッタ対策
                let n = parse_k_option(&chars[pos..], opts)?;
                pos += n;
            }
            b'j' => {
                // -j[n]
                let n = parse_j_option(&chars[pos..], opts);
                pos += n;
            }
            // ダミーオプション（無視）
            b'a' => opts.compat_sw_a = true,
            b'q' => opts.compat_sw_q = true,
            b'z' | b'r' => {}
            _ => {
                return Err(ParseError::Usage(format!(
                    "不明なオプション: -{}",
                    ch as char
                )));
            }
        }
    }

    Ok(consumed)
}

fn parse_t_option(
    chars: &[u8],
    remaining_args: &[Vec<u8>],
    already_consumed: usize,
    pos: usize,
    opts: &mut Options,
) -> Result<usize, ParseError> {
    let (s, n) = get_cmd_string(chars, remaining_args, already_consumed, pos)?;
    opts.temp_path = Some(s);
    Ok(n)
}

fn parse_o_option(
    chars: &[u8],
    remaining_args: &[Vec<u8>],
    already_consumed: usize,
    pos: usize,
    opts: &mut Options,
) -> Result<usize, ParseError> {
    let (mut name, n) = get_cmd_string(chars, remaining_args, already_consumed, pos)?;
    // 拡張子がなければ .o を付ける
    if !name.contains(&b'.') {
        name.extend_from_slice(b".o");
    }
    opts.object_file = Some(name);
    Ok(n)
}

fn parse_i_option(
    chars: &[u8],
    remaining_args: &[Vec<u8>],
    already_consumed: usize,
    pos: usize,
    inc_list: &mut Vec<Vec<u8>>,
) -> Result<usize, ParseError> {
    let (path, n) = get_cmd_string(chars, remaining_args, already_consumed, pos)?;
    inc_list.push(path);
    Ok(n)
}

fn parse_p_option(
    chars: &[u8],
    remaining_args: &[Vec<u8>],
    already_consumed: usize,
    pos: usize,
    opts: &mut Options,
) -> usize {
    opts.make_prn = true;
    let (file, n) = get_optional_filename(&chars[pos..], remaining_args, already_consumed);
    if let Some(f) = file {
        opts.prn_file = Some(f);
    }
    n
}

fn parse_x_option(
    chars: &[u8],
    remaining_args: &[Vec<u8>],
    already_consumed: usize,
    pos: usize,
    opts: &mut Options,
) -> usize {
    opts.make_sym = true;
    let (file, n) = get_optional_filename(&chars[pos..], remaining_args, already_consumed);
    if let Some(f) = file {
        opts.sym_file = Some(f);
    }
    n
}

fn parse_w_option(chars: &[u8], opts: &mut Options) -> Result<usize, ParseError> {
    let (level, n) = get_optional_num(chars);
    opts.warn_level = match level {
        Some(v) if v <= 4 => v as i8,
        Some(_) => return Err(ParseError::Usage("-w: レベルは0-4".into())),
        None => 2,
    };
    Ok(n)
}

fn parse_m_option(
    chars: &[u8],
    remaining_args: &[Vec<u8>],
    already_consumed: usize,
    pos: usize,
    opts: &mut Options,
) -> Result<usize, ParseError> {
    let (s, n) = get_cmd_string(chars, remaining_args, already_consumed, pos)?;
    let num_str = utils::bytes_to_string(&s);
    let num: u32 = num_str.trim().parse().unwrap_or(0);
    if let Some(cpu) = cpu_number_to_type(num) {
        opts.cpu = cpu;
    } else if num > 1000 && num < 32768 {
        // 最大シンボル数指定（無視）
    } else {
        return Err(ParseError::Usage(format!(
            "-m: 不正なCPU指定 '{}'",
            num_str
        )));
    }
    Ok(n)
}

fn parse_s_option(
    chars: &[u8],
    remaining_args: &[Vec<u8>],
    already_consumed: usize,
    pos: usize,
    opts: &mut Options,
) -> Result<(usize, usize, bool), ParseError> {
    let rest = &chars[pos..];
    if !rest.is_empty() && rest[0].is_ascii_digit() {
        let d = (rest[0] - b'0') as u16;
        if d == 0 || d > 4 {
            return Err(ParseError::Usage("-s: 1-4を指定してください".into()));
        }
        opts.local_len_max = d;
        opts.local_num_max = [10, 100, 1000, 10000][(d - 1) as usize];
        Ok((1, 0, false))
    } else {
        // -s symbol[=n] もしくは次の引数
        let (sym_str, n) = get_cmd_string(chars, remaining_args, already_consumed, pos)?;
        let (name, val) = parse_symbol_def(&sym_str)?;
        opts.symbol_defs.push((name, val));
        Ok((0, n, true))
    }
}

fn parse_y_option(chars: &[u8], opts: &mut Options) -> Result<usize, ParseError> {
    let n = parse_optional_01(chars);
    match n.1 {
        Some(1) | None => opts.predefine = true,
        Some(0) => opts.predefine = false,
        _ => return Err(ParseError::Usage("-y: 0または1を指定".into())),
    }
    Ok(n.0)
}

fn parse_k_option(chars: &[u8], opts: &mut Options) -> Result<usize, ParseError> {
    let n = parse_optional_01(chars);
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
    Ok(n.0)
}

fn parse_j_option(chars: &[u8], opts: &mut Options) -> usize {
    let (val, n) = get_optional_num(chars);
    let v = val.unwrap_or(0xFF) as u8;
    opts.ow_set = (v & 1) != 0;
    opts.ow_offsym = (v & 2) != 0;
    n
}

/// `-c` オプションを解析。返値は消費文字数。
fn parse_c_option(chars: &[u8], opts: &mut Options) -> Result<usize, ParseError> {
    if chars.is_empty() {
        // -c のみ → v2互換（-c2 と同じ）
        apply_c2(opts);
        return Ok(0);
    }
    match chars[0] {
        b'0' => {
            apply_c0(opts);
            Ok(1)
        }
        b'1' => {
            apply_c1(opts);
            Ok(1)
        }
        b'2' => {
            apply_c2(opts);
            Ok(1)
        }
        b'3' => {
            apply_c3(opts);
            Ok(1)
        }
        b'4' => {
            apply_c4(opts);
            Ok(1)
        }
        _ => {
            // -c<mnemonic> を解析（fscc / movep / all）
            let end = chars
                .iter()
                .position(|&c| !c.is_ascii_alphanumeric() && c != b'_')
                .unwrap_or(chars.len());
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
                _ => {
                    return Err(ParseError::Usage(format!(
                        "-c: 不明なニーモニック '{}'",
                        utils::bytes_to_string(&mnem)
                    )))
                }
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
        None => {
            opts.prn_no_page_ff = true;
            return pos;
        }
        Some(0) => opts.prn_no_page_ff = true,
        Some(1) => opts.prn_no_page_ff = false,
        _ => {
            opts.prn_no_page_ff = true;
            return pos;
        }
    }
    if chars.get(pos) != Some(&b',') {
        return pos;
    }
    pos += 1;
    // m（マクロ展開）
    let (v, n) = get_optional_num(&chars[pos..]);
    pos += n;
    if let Some(v) = v {
        opts.prn_is_lall = v != 0;
    }
    if chars.get(pos) != Some(&b',') {
        return pos;
    }
    pos += 1;
    // w（幅）
    let (v, n) = get_optional_num(&chars[pos..]);
    pos += n;
    if let Some(v) = v {
        if (80..256).contains(&v) {
            opts.prn_width = (v & !7) as u16;
        }
    }
    if chars.get(pos) != Some(&b',') {
        return pos;
    }
    pos += 1;
    // p（ページ行数）
    let (v, n) = get_optional_num(&chars[pos..]);
    pos += n;
    if let Some(v) = v {
        if (10..256).contains(&v) {
            opts.prn_page_lines = v as u16;
        }
    }
    if chars.get(pos) != Some(&b',') {
        return pos;
    }
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
    pos: usize,
) -> Result<(Vec<u8>, usize), ParseError> {
    if pos < chars.len() {
        // スイッチに続く文字列（例: -tpath）
        return Ok((chars[pos..].to_vec(), 0));
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
