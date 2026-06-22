use super::encode::{encode_ea, EaEncoded, EncodeError};
use super::{
    ea, eac, Displacement, EaError, EffectiveAddress, IdxSize, IndexSpec, Scale,
};
use crate::options::cpu;
use crate::symbol::SymbolTable;

fn make_sym() -> SymbolTable {
    SymbolTable::new(false)
}

fn parse(s: &str) -> EffectiveAddress {
    let sym = make_sym();
    let mut pos = 0;
    super::parse_ea(s.as_bytes(), &mut pos, &sym, cpu::C000).expect(s)
}

fn parse_err(s: &str) -> EaError {
    let sym = make_sym();
    let mut pos = 0;
    super::parse_ea(s.as_bytes(), &mut pos, &sym, cpu::C000).expect_err(s)
}

fn parse_and_encode(s: &str, op_size: u8) -> EaEncoded {
    let sym = make_sym();
    let mut pos = 0;
    let ea = super::parse_ea(s.as_bytes(), &mut pos, &sym, cpu::C000).expect(s);
    encode_ea(&ea, op_size).unwrap_or_else(|_| panic!("encode {}", s))
}

// =================================================================
// mod.rs テスト群
// =================================================================

// ---- レジスタ直接 ----

#[test]
fn test_data_reg() {
    for i in 0..8u8 {
        let s = format!("d{}", i);
        assert_eq!(parse(&s), EffectiveAddress::DataReg(i), "{}", s);
    }
    assert_eq!(parse("D3"), EffectiveAddress::DataReg(3));
}

#[test]
fn test_addr_reg() {
    for i in 0..8u8 {
        let s = format!("a{}", i);
        assert_eq!(parse(&s), EffectiveAddress::AddrReg(i), "{}", s);
    }
    assert_eq!(parse("sp"), EffectiveAddress::AddrReg(7));
    assert_eq!(parse("SP"), EffectiveAddress::AddrReg(7));
}

// ---- アドレスレジスタ間接 ----

#[test]
fn test_addr_reg_ind() {
    assert_eq!(parse("(a0)"), EffectiveAddress::AddrRegInd(0));
    assert_eq!(parse("(a5)"), EffectiveAddress::AddrRegInd(5));
    assert_eq!(parse("( a0 )"), EffectiveAddress::AddrRegInd(0));
}

#[test]
fn test_post_inc() {
    assert_eq!(parse("(a0)+"), EffectiveAddress::AddrRegPostInc(0));
    assert_eq!(parse("(a7)+"), EffectiveAddress::AddrRegPostInc(7));
}

#[test]
fn test_pre_dec() {
    assert_eq!(parse("-(a0)"), EffectiveAddress::AddrRegPreDec(0));
    assert_eq!(parse("-(sp)"), EffectiveAddress::AddrRegPreDec(7));
}

// ---- ディスプレースメント付きアドレスレジスタ間接 ----

fn disp_val(ea: &EffectiveAddress) -> i32 {
    match ea {
        EffectiveAddress::AddrRegDisp { disp, .. } => {
            // RPN を評価して定数を得る
            crate::expr::eval_rpn(&disp.rpn, 0, 0, 0, &|_| None)
                .unwrap()
                .value
        }
        _ => panic!("not AddrRegDisp"),
    }
}

fn disp_an(ea: &EffectiveAddress) -> u8 {
    match ea {
        EffectiveAddress::AddrRegDisp { an, .. } => *an,
        _ => panic!("not AddrRegDisp"),
    }
}

#[test]
fn test_addr_reg_disp() {
    // 括弧内形式 (d,An)
    let ea = parse("(4,a0)");
    assert!(matches!(ea, EffectiveAddress::AddrRegDisp { an: 0, .. }));
    assert_eq!(disp_val(&ea), 4);

    // 前置形式 d(An)
    let ea2 = parse("4(a0)");
    assert!(matches!(ea2, EffectiveAddress::AddrRegDisp { an: 0, .. }));
    assert_eq!(disp_val(&ea2), 4);

    // 負のディスプレースメント
    let ea3 = parse("(-8,a5)");
    assert_eq!(disp_an(&ea3), 5);
    assert_eq!(disp_val(&ea3), -8);
}

#[test]
fn test_addr_reg_disp_zero() {
    // (0,An) は (An) と同じではなく AddrRegDisp として解析される
    let ea = parse("(0,a3)");
    assert!(matches!(ea, EffectiveAddress::AddrRegDisp { an: 3, .. }));
}

// ---- インデックス付きアドレスレジスタ間接 ----

#[test]
fn test_addr_reg_idx_basic() {
    // (0,a0,d1) → AddrRegIdx
    let ea = parse("(0,a0,d1)");
    match ea {
        EffectiveAddress::AddrRegIdx { an, ref idx, .. } => {
            assert_eq!(an, 0);
            assert_eq!(idx.reg, 1);
            assert_eq!(idx.size, IdxSize::Word);
            assert_eq!(idx.scale, Scale::S1);
        }
        _ => panic!("expected AddrRegIdx"),
    }
}

#[test]
fn test_addr_reg_idx_long() {
    let ea = parse("(2,a3,d4.l)");
    match ea {
        EffectiveAddress::AddrRegIdx { an, ref idx, .. } => {
            assert_eq!(an, 3);
            assert_eq!(idx.reg, 4);
            assert_eq!(idx.size, IdxSize::Long);
        }
        _ => panic!("expected AddrRegIdx"),
    }
}

#[test]
fn test_addr_reg_idx_an_index() {
    // インデックスレジスタに An を使う
    let ea = parse("(0,a0,a1.w)");
    match ea {
        EffectiveAddress::AddrRegIdx { an, ref idx, .. } => {
            assert_eq!(an, 0);
            assert_eq!(idx.reg, 0x08 + 1); // A1
            assert_eq!(idx.size, IdxSize::Word);
        }
        _ => panic!("expected AddrRegIdx"),
    }
}

#[test]
fn test_addr_reg_idx_no_disp() {
    // (a0,d1) → AddrRegIdx with zero displacement
    let ea = parse("(a0,d1)");
    match &ea {
        EffectiveAddress::AddrRegIdx { an, disp, idx } => {
            assert_eq!(*an, 0);
            assert!(disp.is_zero());
            assert_eq!(idx.reg, 1);
        }
        _ => panic!("expected AddrRegIdx, got {:?}", ea),
    }
}

#[test]
fn test_addr_reg_idx_dn_first() {
    // (d1,a0) → AddrRegIdx (Dn が先でも An がベース)
    let ea = parse("(d1,a0)");
    match &ea {
        EffectiveAddress::AddrRegIdx { an, disp, idx } => {
            assert_eq!(*an, 0);
            assert!(disp.is_zero());
            assert_eq!(idx.reg, 1);
        }
        _ => panic!("expected AddrRegIdx, got {:?}", ea),
    }
}

// ---- 絶対アドレス ----

#[test]
fn test_abs_short() {
    let ea = parse("$1234.w");
    assert!(matches!(ea, EffectiveAddress::AbsShort(_)));

    let ea2 = parse("($1234).w");
    assert!(matches!(ea2, EffectiveAddress::AbsShort(_)));
}

#[test]
fn test_abs_long() {
    let ea = parse("$12345678.l");
    assert!(matches!(ea, EffectiveAddress::AbsLong(_)));

    // デフォルト（サイズ指定なし）はロング
    let ea2 = parse("$1000");
    assert!(matches!(ea2, EffectiveAddress::AbsLong(_)));

    let ea3 = parse("($1234).l");
    assert!(matches!(ea3, EffectiveAddress::AbsLong(_)));
}

// ---- PC相対 ----

#[test]
fn test_pc_disp() {
    let ea = parse("(4,pc)");
    assert!(matches!(ea, EffectiveAddress::PcDisp(_)));
}

#[test]
fn test_pc_idx() {
    let ea = parse("(2,pc,d0)");
    assert!(matches!(ea, EffectiveAddress::PcIdx { .. }));
}

// ---- イミディエイト ----

#[test]
fn test_immediate() {
    let ea = parse("#100");
    assert!(matches!(ea, EffectiveAddress::Immediate(_)));
}

#[test]
fn test_immediate_hex() {
    let ea = parse("#$FFFF");
    assert!(matches!(ea, EffectiveAddress::Immediate(_)));
}

// ---- EA ビットマスク ----

#[test]
fn test_ea_bits() {
    assert_eq!(parse("d0").ea_bits(), ea::DN);
    assert_eq!(parse("a0").ea_bits(), ea::AN);
    assert_eq!(parse("(a0)").ea_bits(), ea::ADR);
    assert_eq!(parse("(a0)+").ea_bits(), ea::INCADR);
    assert_eq!(parse("-(a0)").ea_bits(), ea::DECADR);
    assert_eq!(parse("(4,a0)").ea_bits(), ea::DSPADR);
    assert_eq!(parse("(0,a0,d0)").ea_bits(), ea::IDXADR);
    assert_eq!(parse("$1000.w").ea_bits(), ea::ABSW);
    assert_eq!(parse("$1000").ea_bits(), ea::ABSL);
    assert_eq!(parse("(4,pc)").ea_bits(), ea::DSPPC);
    assert_eq!(parse("(0,pc,d0)").ea_bits(), ea::IDXPC);
    assert_eq!(parse("#0").ea_bits(), ea::IMM);
}

// ---- エラーケース ----

#[test]
fn test_error_empty() {
    assert_eq!(parse_err(""), EaError::ExpectedOperand);
}

// =================================================================
// encode.rs テスト群
// =================================================================

// ---- EA フィールド値の確認 ----

#[test]
fn test_ea_field_dn() {
    assert_eq!(parse_and_encode("d0", 1).ea_field, 0b000_000);
    assert_eq!(parse_and_encode("d7", 1).ea_field, 0b000_111);
}

#[test]
fn test_ea_field_an() {
    assert_eq!(parse_and_encode("a0", 1).ea_field, 0b001_000);
    assert_eq!(parse_and_encode("a3", 1).ea_field, 0b001_011);
    assert_eq!(parse_and_encode("sp", 1).ea_field, 0b001_111);
}

#[test]
fn test_ea_field_adr() {
    assert_eq!(parse_and_encode("(a0)", 1).ea_field, 0b010_000);
    assert_eq!(parse_and_encode("(a5)", 1).ea_field, 0b010_101);
}

#[test]
fn test_ea_field_incadr() {
    assert_eq!(parse_and_encode("(a0)+", 1).ea_field, 0b011_000);
}

#[test]
fn test_ea_field_decadr() {
    assert_eq!(parse_and_encode("-(a0)", 1).ea_field, 0b100_000);
}

// ---- 拡張ワードの確認 ----

#[test]
fn test_encode_dspadr() {
    let enc = parse_and_encode("(4,a3)", 1);
    assert_eq!(enc.ea_field, eac::DSPADR | 3);
    assert_eq!(enc.ext_bytes, vec![0x00, 0x04]); // +4
}

#[test]
fn test_encode_dspadr_negative() {
    let enc = parse_and_encode("(-8,a0)", 1);
    assert_eq!(enc.ea_field, eac::DSPADR);
    assert_eq!(enc.ext_bytes, vec![0xFF, 0xF8]); // -8 as i16 = 0xFFF8
}

#[test]
fn test_encode_idxadr() {
    // (2,a0,d1.w*1) brief extension word:
    // bit15=0(Dn), bits14-12=001(D1), bit11=0(.w), bits10-9=00(*1), bit8=0, bits7-0=0x02
    let enc = parse_and_encode("(2,a0,d1)", 1);
    assert_eq!(enc.ea_field, eac::IDXADR);
    // D1.w*1 disp=2: 0001_0000_0000_0010 = 0x1002
    assert_eq!(enc.ext_bytes, vec![0x10, 0x02]);
}

#[test]
fn test_encode_idxadr_long() {
    // (0,a3,d4.l) brief extension word:
    // D4.l*1 disp=0: bit15=0, 100(D4), 1(.l), 00(*1), 0, 0x00 = 0x4800
    let enc = parse_and_encode("(0,a3,d4.l)", 1);
    assert_eq!(enc.ea_field, eac::IDXADR | 3);
    assert_eq!(enc.ext_bytes, vec![0x48, 0x00]);
}

#[test]
fn test_encode_idxadr_an_index() {
    // (0,a0,a1.w) → A1 as index: bit15=1(An), 001(A1), 0(.w), 00(*1), 0, 0x00 = 0x9000
    let enc = parse_and_encode("(0,a0,a1.w)", 1);
    assert_eq!(enc.ea_field, eac::IDXADR);
    assert_eq!(enc.ext_bytes, vec![0x90, 0x00]);
}

#[test]
fn test_encode_absw() {
    let enc = parse_and_encode("$1234.w", 1);
    assert_eq!(enc.ea_field, eac::ABSW);
    assert_eq!(enc.ext_bytes, vec![0x12, 0x34]);
}

#[test]
fn test_encode_absl() {
    let enc = parse_and_encode("$12345678", 1);
    assert_eq!(enc.ea_field, eac::ABSL);
    assert_eq!(enc.ext_bytes, vec![0x12, 0x34, 0x56, 0x78]);
}

#[test]
fn test_encode_dsppc() {
    let enc = parse_and_encode("(4,pc)", 1);
    assert_eq!(enc.ea_field, eac::DSPPC);
    assert_eq!(enc.ext_bytes, vec![0x00, 0x04]);
}

#[test]
fn test_encode_idxpc() {
    // (2,pc,d0) → PC rel idx, D0.w*1, disp=2
    let enc = parse_and_encode("(2,pc,d0)", 1);
    assert_eq!(enc.ea_field, eac::IDXPC);
    // D0.w*1 disp=2: 0000_0000_0000_0010 = 0x0002
    assert_eq!(enc.ext_bytes, vec![0x00, 0x02]);
}

#[test]
fn test_encode_imm_byte() {
    let enc = parse_and_encode("#$42", 0);
    assert_eq!(enc.ea_field, eac::IMM);
    assert_eq!(enc.ext_bytes, vec![0x00, 0x42]); // byte padded to word
}

#[test]
fn test_encode_imm_word() {
    let enc = parse_and_encode("#$1234", 1);
    assert_eq!(enc.ea_field, eac::IMM);
    assert_eq!(enc.ext_bytes, vec![0x12, 0x34]);
}

#[test]
fn test_encode_imm_long() {
    let enc = parse_and_encode("#$12345678", 2);
    assert_eq!(enc.ea_field, eac::IMM);
    assert_eq!(enc.ext_bytes, vec![0x12, 0x34, 0x56, 0x78]);
}

// ---- 直接構築した EA のエンコード ----

#[test]
fn test_encode_manual_idx() {
    // (4,a5,d2.l*4) のエンコードを直接検証
    let ea = EffectiveAddress::AddrRegIdx {
        an: 5,
        disp: Displacement {
            rpn: vec![],
            size: None,
            const_val: Some(4),
        },
        idx: IndexSpec {
            reg: 2,
            size: IdxSize::Long,
            scale: Scale::S4,
            suppress: false,
        },
    };
    let enc = encode_ea(&ea, 1).unwrap();
    assert_eq!(enc.ea_field, eac::IDXADR | 5);
    // D2.l*4: bit15=0(Dn), 010(D2), 1(.l), 10(*4), 0, disp=4 = 0010_1100_0000_0100 = 0x2C04
    assert_eq!(enc.ext_bytes, vec![0x2C, 0x04]);
}

// ---- エラーケース ----

#[test]
fn test_encode_disp_overflow_uses_full_format() {
    // 68020+: 16ビット超のディスプレースメントはフルフォーマット拡張ワードで処理
    let ea = EffectiveAddress::AddrRegDisp {
        an: 0,
        disp: Displacement {
            rpn: vec![],
            size: None,
            const_val: Some(0x10000),
        },
    };
    let enc = encode_ea(&ea, 1).unwrap();
    assert_eq!(enc.ea_field, eac::IDXADR); // mode 110, reg A0
                                           // ext word (0x0170) + long BD (4 bytes) = 6 bytes
    assert_eq!(enc.ext_bytes.len(), 6);
    assert_eq!(enc.ext_bytes[0], 0x01); // full format: IS=1, BD=long
    assert_eq!(enc.ext_bytes[1], 0x70);
}

#[test]
fn test_encode_brief_disp_overflow() {
    let ea = EffectiveAddress::AddrRegIdx {
        an: 0,
        disp: Displacement {
            rpn: vec![],
            size: None,
            const_val: Some(200),
        }, // > 127
        idx: IndexSpec {
            reg: 0,
            size: IdxSize::Word,
            scale: Scale::S1,
            suppress: false,
        },
    };
    assert!(matches!(
        encode_ea(&ea, 1),
        Err(EncodeError::DisplacementOutOfRange { bits: 8, .. })
    ));
}
