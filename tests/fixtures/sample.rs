pub struct Config {
    pub name: String,
    pub retries: u32,
}

impl Config {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            retries: 3,
        }
    }

    pub fn validate(&self) -> bool {
        if self.name.is_empty() {
            return false;
        }
        if self.retries == 0 {
            return false;
        }
        true
    }
}

fn fibonacci(n: u32) -> u64 {
    match n {
        0 => 0,
        1 => 1,
        _ => fibonacci(n - 1) + fibonacci(n - 2),
    }
}
