use super::*;

#[test]
fn test_bytes_to_string() {
    let b = b"hello";
    assert_eq!(bytes_to_string(b), "hello");
}

#[test]
fn test_bytes_to_string_with_non_ascii() {
    // 有効な UTF-8
    let b = "こんにちは".as_bytes();
    assert_eq!(bytes_to_string(b), "こんにちは");
}

#[test]
fn test_path_from_bytes() {
    let b = b"output.o";
    let p = path_from_bytes(b);
    assert_eq!(p.to_string_lossy(), "output.o");
}

#[test]
fn test_to_lowercase_vec() {
    let b = b"MOVE";
    assert_eq!(to_lowercase_vec(b), b"move".to_vec());
}

#[test]
fn test_to_lowercase_vec_mixed() {
    let b = b"MoVe";
    assert_eq!(to_lowercase_vec(b), b"move".to_vec());
}

#[test]
fn test_to_lowercase_vec_already_lower() {
    let b = b"move";
    assert_eq!(to_lowercase_vec(b), b"move".to_vec());
}

#[test]
fn test_to_lowercase_buf() {
    let mut b = b"MOVE".to_vec();
    to_lowercase_buf(&mut b);
    assert_eq!(b, b"move".to_vec());
}
