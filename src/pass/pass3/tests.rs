use super::ea::*;
use crate::expr::rpn::{Operator, RPNToken};
use crate::symbol::SymbolTable;

#[test]
fn test_is_external_with_offset_mul_add_const_fold() {
    let sym = SymbolTable::new(false);
    let rpn = vec![
        RPNToken::SymbolRef(b"extsym".to_vec()),
        RPNToken::Value(16),
        RPNToken::Value(4),
        RPNToken::Op(Operator::Mul),
        RPNToken::Op(Operator::Add),
        RPNToken::End,
    ];
    let got = is_external_with_offset(&rpn, &sym).expect("ext + 16*4");
    assert_eq!(got.0, b"extsym".to_vec());
    assert_eq!(got.1, 64);
}
