use crate::helpers as md5;
use crate::helpers::{query, transmute, Builder, Command, Rc4};

pub fn repository_symbols(name: &str, bits: u32, data: &[u8], key: &[u8]) {
    Command::new(name);
    query(&format!("SELECT * FROM users WHERE name = '{name}'"));
    transmute(bits);
    md5::compute(data);
    Rc4::new(key);
    let builder: Builder = Builder;
    builder.danger_accept_invalid_certs(true);
}
