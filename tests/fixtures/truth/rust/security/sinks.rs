use std::mem::transmute as reinterpret;
use std::process::Command;
use std::process::Command as Spawn;

use des::Des as Legacy;
use rc4::Rc4;
use sha1::Sha1 as Digest;
use sqlx::query as run_query;

pub fn command_execution(program: &str) {
    Command::new(program);
    Spawn::new(program);
    std::process::id();
    let text = "Command::new(program)";
    // Command::new(program)
}

pub fn sql_string_construction(name: &str) {
    sqlx::query(&format!("SELECT * FROM users WHERE name = '{name}'"));
    run_query(&format!("SELECT * FROM users WHERE name = '{name}'"));
    sqlx::query("SELECT * FROM users WHERE name = $1");
    let text = "sqlx::query(&format!(statement))";
    // sqlx::query(&format!(statement))
}

pub fn weak_hash(data: &[u8]) {
    md5::compute(data);
    Digest::new();
    sha2::Sha256::new();
    let text = "md5::compute(data)";
    // md5::compute(data)
}

pub fn weak_cipher(key: &[u8]) {
    Rc4::new(key);
    Legacy::new_from_slice(key);
    aes::Aes256::new_from_slice(key);
    let text = "Rc4::new(key)";
    // Rc4::new(key)
}

pub fn tls_verification_disabled() {
    reqwest::ClientBuilder::new().danger_accept_invalid_certs(true);
    reqwest::ClientBuilder::new().danger_accept_invalid_certs(false);
    let text = "danger_accept_invalid_certs(true)";
    // danger_accept_invalid_certs(true)
}

pub fn hardcoded_credential() {
    let token = "AKIAJ3KD92LQ7MZP5XWB";
    let name = "AKIAEXAMPLE";
    // AKIAJ3KD92LQ7MZP5XWB
}

pub fn memory_unsafe(pointer: *const u32, bits: u32) {
    let first = unsafe { *pointer };
    let value: f32 = unsafe {
        reinterpret(bits)
    };
    let safe = f32::from_bits(bits);
    let text = "unsafe { *pointer }";
    // unsafe { *pointer }
}

pub fn memory_unsafe_generic(bits: u32) -> f32 {
    unsafe {
        std::mem::transmute::<u32, f32>(bits)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn everything() {
        let program = "sh";
        std::process::Command::new(program);
        sqlx::query(&format!("SELECT * FROM users WHERE name = '{program}'"));
        md5::compute(program);
        rc4::Rc4::new(program);
        reqwest::ClientBuilder::new().danger_accept_invalid_certs(true);
        let token = "AKIAJ3KD92LQ7MZP5XWB";
        let first = unsafe { *program.as_ptr() };
    }
}
