use std::process::Command;

pub fn cleanup() {
    let _ = Command::new(std::env::var("TARGET").unwrap()).output();
}
