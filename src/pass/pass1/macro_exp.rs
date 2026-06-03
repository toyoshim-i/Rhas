use super::{
    parse_line, parse_string_or_ident, read_ident, should_emit_line_info, skip_spaces, P1Ctx,
};
use crate::context::AssemblyContext;
use crate::expr::rpn::RPNToken;
use crate::expr::{parse_expr, Rpn};
use crate::pass::temp::TempRecord;
use crate::source::{ReadResult, SourceStack};
use crate::symbol::types::InsnHandler;
use crate::symbol::{Symbol, SymbolTable};

pub(crate) fn parse_macro_params(line: &[u8], pos: &mut usize) -> Vec<Vec<u8>> {
    let mut params = Vec::new();
    skip_spaces(line, pos);
    while *pos < line.len() && line[*pos] != b';' && line[*pos] != b'*' {
        let p = read_ident(line, pos);
        if p.is_empty() {
            break;
        }
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
    if *pos >= line.len() {
        return Vec::new();
    }
    if line[*pos] == b'<' {
        *pos += 1;
        let mut arg = Vec::new();
        let mut nest = 1u32;
        while *pos < line.len() {
            let b = line[*pos];
            *pos += 1;
            if b == b'<' {
                nest += 1;
                arg.push(b);
            } else if b == b'>' {
                nest -= 1;
                if nest == 0 {
                    break;
                }
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
            if b == b',' || b == b';' || b == b'\n' {
                break;
            }
            *pos += 1;
        }
        let end = *pos;
        let s = &line[start..end];
        s.iter()
            .rev()
            .skip_while(|&&b| b == b' ' || b == b'\t')
            .count();
        let trim_end = end
            - s.iter()
                .rev()
                .take_while(|&&b| b == b' ' || b == b'\t')
                .count();
        line[start..trim_end].to_vec()
    }
}

pub(crate) fn collect_macro_body(
    source: &mut SourceStack,
    sym: &SymbolTable,
    ctx: &mut AssemblyContext,
    params: &[Vec<u8>],
) -> (Vec<u8>, u16) {
    let mut template = Vec::new();
    let mut local_count = 0u16;
    let mut nest_depth = 0u32;
    let mut name_map: std::collections::HashMap<Vec<u8>, u16> = std::collections::HashMap::new();

    while let ReadResult::Line(line) = source.read_line() {
        let trim_len = line
            .iter()
            .rev()
            .take_while(|&&b| b == b'\r' || b == b'\n')
            .count();
        let line = &line[..line.len() - trim_len];

        let mnem = extract_mnemonic(line);
        let handler = sym.lookup_cmd(&mnem, ctx.cpu.features).and_then(|s| {
            if let Symbol::Opcode { handler, .. } = s {
                Some(*handler)
            } else {
                None
            }
        });

        match handler {
            Some(
                InsnHandler::MacroDef | InsnHandler::Rept | InsnHandler::Irp | InsnHandler::Irpc,
            ) => {
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
                    let converted =
                        convert_line_params(line, params, &mut local_count, &mut name_map);
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
                p.len() == name.len() && p.iter().zip(name).all(|(a, b)| a.eq_ignore_ascii_case(b))
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
        if b == b'@' && i + 1 < line.len() && line[i + 1] != b'@' {
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
                        p.len() == name.len()
                            && p.iter().zip(name).all(|(a, b2)| a.eq_ignore_ascii_case(b2))
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
            if i < line.len() {
                out.push(line[i]);
                i += 1;
            }
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
                    p.len() == name.len()
                        && p.iter()
                            .zip(name.iter())
                            .all(|(a, b2)| a.eq_ignore_ascii_case(b2))
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

pub(crate) fn expand_macro_body(
    template: &[u8],
    params: &[Vec<u8>],
    args: &[Vec<u8>],
    local_base: u32,
    records: &mut Vec<TempRecord>,
    p1: &mut P1Ctx<'_>,
    source: &mut SourceStack,
) {
    let mut start = 0;
    while start <= template.len() {
        let end = template[start..]
            .iter()
            .position(|&b| b == b'\n')
            .map(|n| start + n)
            .unwrap_or(template.len());
        if end == start && start == template.len() {
            break;
        }

        let tline = &template[start..end];
        let next_start = if end < template.len() {
            end + 1
        } else {
            template.len()
        };

        let expanded = expand_line(tline, params, args, local_base, p1.sym);

        let mnem = extract_mnemonic(&expanded);
        let handler_opt = p1.sym.lookup_cmd(&mnem, p1.cpu_type()).and_then(|s| {
            if let Symbol::Opcode { handler, .. } = s {
                Some(*handler)
            } else {
                None
            }
        });

        match handler_opt {
            Some(InsnHandler::Rept) => {
                let remaining = &template[next_start..];
                let (body, _, consumed) = collect_body_from_slice(remaining, p1.sym, p1.ctx);
                start = next_start + consumed;

                if !p1.is_skip {
                    let line = &expanded;
                    let mut pos = 0usize;
                    skip_spaces(line, &mut pos);
                    while pos < line.len() && !line[pos].is_ascii_whitespace() {
                        pos += 1;
                    }
                    skip_spaces(line, &mut pos);
                    let count = if let Ok(rpn) = parse_expr(line, &mut pos) {
                        p1.eval_const(&rpn).map(|v| v.value as u32).unwrap_or(0)
                    } else {
                        0
                    };
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
                while pos < line.len() && !line[pos].is_ascii_whitespace() {
                    pos += 1;
                }
                skip_spaces(line, &mut pos);
                let param_name = read_ident(line, &mut pos);
                skip_spaces(line, &mut pos);
                if pos < line.len() && line[pos] == b',' {
                    pos += 1;
                }
                let irp_args = parse_macro_args(line, &mut pos);
                let irp_params = if param_name.is_empty() {
                    vec![]
                } else {
                    vec![param_name]
                };
                let (body, _, consumed) =
                    collect_body_from_slice_with_params(remaining, p1.sym, p1.ctx, &irp_params);
                start = next_start + consumed;

                if !p1.is_skip {
                    for irp_arg in &irp_args {
                        let lb = p1.next_local_base();
                        expand_macro_body(
                            &body,
                            &irp_params,
                            std::slice::from_ref(irp_arg),
                            lb,
                            records,
                            p1,
                            source,
                        );
                    }
                }
                continue;
            }
            Some(InsnHandler::Irpc) => {
                let remaining = &template[next_start..];
                let line = &expanded;
                let mut pos = 0usize;
                while pos < line.len() && !line[pos].is_ascii_whitespace() {
                    pos += 1;
                }
                skip_spaces(line, &mut pos);
                let param_name = read_ident(line, &mut pos);
                skip_spaces(line, &mut pos);
                if pos < line.len() && line[pos] == b',' {
                    pos += 1;
                }
                skip_spaces(line, &mut pos);
                let s = parse_string_or_ident(line, &mut pos);
                let irpc_params = if param_name.is_empty() {
                    vec![]
                } else {
                    vec![param_name]
                };
                let (body, _, consumed) =
                    collect_body_from_slice_with_params(remaining, p1.sym, p1.ctx, &irpc_params);
                start = next_start + consumed;

                if !p1.is_skip {
                    for &ch in &s {
                        let arg = vec![ch];
                        let lb = p1.next_local_base();
                        expand_macro_body(
                            &body,
                            &irpc_params,
                            std::slice::from_ref(&arg),
                            lb,
                            records,
                            p1,
                            source,
                        );
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
        let end = slice[pos..]
            .iter()
            .position(|&b| b == b'\n')
            .map(|n| pos + n)
            .unwrap_or(slice.len());
        let line = &slice[pos..end];
        let next_pos = if end < slice.len() {
            end + 1
        } else {
            slice.len()
        };

        let mnem = extract_mnemonic(line);
        let handler = sym.lookup_cmd(&mnem, ctx.cpu.features).and_then(|s| {
            if let Symbol::Opcode { handler, .. } = s {
                Some(*handler)
            } else {
                None
            }
        });

        match handler {
            Some(
                InsnHandler::MacroDef | InsnHandler::Rept | InsnHandler::Irp | InsnHandler::Irpc,
            ) => {
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
                    let converted =
                        convert_line_params(line, params, &mut local_count, &mut name_map);
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
    tline: &[u8],
    _params: &[Vec<u8>],
    args: &[Vec<u8>],
    local_base: u32,
    sym: &SymbolTable,
) -> Vec<u8> {
    let mut out = Vec::with_capacity(tline.len() + 16);
    let mut i = 0;
    while i < tline.len() {
        let b = tline[i];
        if b == 0xFF && i + 2 < tline.len() {
            let idx = ((tline[i + 1] as usize) << 8) | (tline[i + 2] as usize);
            i += 3;
            if let Some(arg) = args.get(idx) {
                out.extend_from_slice(arg);
            }
        } else if b == 0xFE && i + 2 < tline.len() {
            let lno = ((tline[i + 1] as u32) << 8) | (tline[i + 2] as u32);
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
    while pos < line.len() && (line[pos] == b' ' || line[pos] == b'\t') {
        pos += 1;
    }
    if pos < line.len() && line[pos] == b'.' {
        pos += 1;
    }
    let start = pos;
    while pos < line.len() && (line[pos].is_ascii_alphanumeric() || line[pos] == b'_') {
        pos += 1;
    }
    line[start..pos].to_ascii_lowercase()
}
