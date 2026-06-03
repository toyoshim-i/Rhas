use std::collections::HashMap;

/// 匿名ローカルラベル（@@: / @b / @f）を行内で展開する。
/// @@: → @@{count}: に展開（is_anon_def=true を返す）
/// @b → @@{count-1}、@f → @@{count} に展開する。
/// コメント（;）以降は処理しない。
pub(super) fn preprocess_anon_labels(line: &[u8], count: u32) -> (Vec<u8>, bool) {
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
pub(super) fn preprocess_numeric_local_labels(
    line: &[u8],
    counts: &mut HashMap<u32, u32>,
) -> Vec<u8> {
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
            let left_boundary =
                (i == 0 || !is_num_local_ident_cont(line[i - 1])) && !numeric_prefix;
            if left_boundary {
                let mut k = i;
                while k < line.len() && line[k].is_ascii_digit() {
                    k += 1;
                }
                let suffix = line.get(k).copied();
                if let Some(suffix_char @ (b'f' | b'b')) = suffix {
                    let after = k + 1;
                    let right_boundary =
                        after >= line.len() || !is_num_local_ident_cont(line[after]);
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
