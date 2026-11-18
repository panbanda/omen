#[cfg(unix)]
fn main() {
    let count = tools::init();
    println!("{count}");
}

#[cfg(not(unix))]
fn main() {}
