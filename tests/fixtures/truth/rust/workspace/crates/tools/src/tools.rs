pub mod command;

pub fn init() -> usize {
    get()
}

pub fn get() -> usize {
    1
}

#[cfg(unix)]
pub fn extract_zip() -> usize {
    1
}

#[cfg(not(unix))]
pub fn extract_zip() -> usize {
    2
}

pub fn unpack() -> usize {
    extract_zip()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unpacks() {
        let size = extract_zip();
        assert_eq!(size, unpack());
    }
}

pub mod runner {
    use super::command::new_command;

    pub fn launch() -> usize {
        new_command()
    }
}
