fn shared() -> u32 {
    3
}

#[test]
fn runs() {
    assert_eq!(shared(), 3);
}
