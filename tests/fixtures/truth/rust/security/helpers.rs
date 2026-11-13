pub struct Builder;

impl Builder {
    pub fn danger_accept_invalid_certs(&self, accept: bool) -> bool {
        accept
    }
}

pub fn query(statement: &str) -> &str {
    statement
}

pub fn transmute(bits: u32) -> u32 {
    bits
}

pub struct Rc4;

impl Rc4 {
    pub fn new(key: &[u8]) -> usize {
        key.len()
    }
}

pub struct Command;

impl Command {
    pub fn new(program: &str) -> usize {
        program.len()
    }
}

pub fn compute(data: &[u8]) -> usize {
    data.len()
}
