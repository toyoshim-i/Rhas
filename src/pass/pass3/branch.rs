use super::P3Ctx;
use crate::expr::Rpn;
use crate::symbol::types::SizeCode;

pub(super) fn process_branch(
    ctx: &mut P3Ctx<'_>,
    opcode: u16,
    target: &Rpn,
    req_size: Option<SizeCode>,
    suppressed: bool,
) {
    if suppressed {
        return;
    }
    let pc = ctx.location(); // 命令先頭のアドレス

    // ターゲットアドレスを評価
    let target_addr = match ctx.eval(target) {
        Ok(v) => v.value,
        Err(e) => {
            // 外部参照 → PC相対リロケーションレコードを生成
            let xref_num = ctx.get_or_add_xref(e);
            match req_size {
                Some(SizeCode::Long) => {
                    // .l 形式の外部参照: RPN リロケーション
                    // opcode = $xxFF (long form indicator), 4バイトディスプレースメント
                    ctx.emit(&[(opcode >> 8) as u8, 0xFF]);
                    let base_addr = ctx.location(); // ディスプレースメント基準 = pc + 2
                    ctx.advance(4);
                    ctx.flush_code_buf();
                    ctx.flush_dsb();
                    super::emit_pc_rel_rpn(ctx, xref_num, base_addr, 0x92);
                }
                Some(SizeCode::Short) => {
                    // .s 形式: オペコードのみ出力、ディスプレースメントはリンカが提供
                    ctx.emit(&[(opcode >> 8) as u8, 0x00]);
                    // 命令長は2バイト。ディスプレースメントはオペコードに埋め込まれる（1バイト）
                    // loc = pc+1（ディスプレースメントバイトのアドレス）
                    let loc = pc + 1;
                    ctx.flush_code_buf();
                    ctx.flush_dsb();
                    let sect = ctx.cur_sect;
                    ctx.code_body.extend_from_slice(&[0x6B, sect]);
                    ctx.code_body.extend_from_slice(&loc.to_be_bytes());
                    ctx.code_body.push((xref_num >> 8) as u8);
                    ctx.code_body.push(xref_num as u8);
                }
                _ => {
                    // .w 形式 (デフォルト): オペコード2バイト + リンカ提供ディスプレースメント2バイト
                    ctx.emit(&[(opcode >> 8) as u8, 0x00]);
                    let loc = ctx.location(); // pc + 2 = ディスプレースメントスロットのアドレス
                    ctx.advance(2); // リンカが2バイトのディスプレースメントを提供
                    ctx.flush_code_buf();
                    ctx.flush_dsb();
                    let sect = ctx.cur_sect;
                    ctx.code_body.extend_from_slice(&[0x65, sect]);
                    ctx.code_body.extend_from_slice(&loc.to_be_bytes());
                    ctx.code_body.push((xref_num >> 8) as u8);
                    ctx.code_body.push(xref_num as u8);
                }
            }
            return;
        }
    };

    // オフセット計算 (target - (pc + 2))
    let offset_w = target_addr - (pc as i32 + 2);

    match req_size {
        Some(SizeCode::Short) => {
            // .s 形式: 2バイト (オフセットを下位バイトに埋め込む)
            if (-128..=127).contains(&offset_w) {
                let b1 = (opcode | (offset_w as u16 & 0xFF)) as u8;
                let b0 = (opcode >> 8) as u8;
                ctx.emit(&[b0, b1]);
            } else {
                ctx.emit_zeros(2);
                ctx.error_code(crate::error::ErrorCode::IlRelOutside, None);
            }
        }
        Some(SizeCode::Long) => {
            // .l 形式: 6バイト
            // オペコード = $xxFF, その後32bitオフセット
            let mut bytes = Vec::with_capacity(6);
            bytes.push((opcode >> 8) as u8);
            bytes.push(0xFF); // long form indicator
            let off_long = target_addr - (pc as i32 + 2 + 4);
            bytes.push((off_long >> 24) as u8);
            bytes.push((off_long >> 16) as u8);
            bytes.push((off_long >> 8) as u8);
            bytes.push(off_long as u8);
            ctx.emit(&bytes);
        }
        _ => {
            // .w 形式 (デフォルト): 4バイト
            if (-32768..=32767).contains(&offset_w) {
                let w = offset_w as i16 as u16;
                let b0 = (opcode >> 8) as u8;
                let b1 = opcode as u8; // 下位バイト is 0x00 for .w
                ctx.emit(&[b0, b1, (w >> 8) as u8, w as u8]);
            } else {
                ctx.emit_zeros(4);
                ctx.error_code(crate::error::ErrorCode::IlRelOutside, None);
            }
        }
    }
}

pub(super) fn val_to_bytes(v: i32, size: u8) -> Vec<u8> {
    match size {
        1 => vec![v as u8],
        2 => {
            let w = v as u16;
            vec![(w >> 8) as u8, w as u8]
        }
        4 => {
            let l = v as u32;
            vec![(l >> 24) as u8, (l >> 16) as u8, (l >> 8) as u8, l as u8]
        }
        _ => vec![],
    }
}
