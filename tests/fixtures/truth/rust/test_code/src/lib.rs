pub fn production() -> u32 {
    1
}

pub fn test_connection() -> u32 {
    production()
}

#[perf]
pub fn measured_in_production() {}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
// The module has two attributes and a comment before it.
mod checks {
    use super::*;

    fn helper() -> u32 {
        production()
    }

    #[test]
    fn plain() {
        assert_eq!(helper(), 1);
    }

    #[test]
    /// The documentation is between the attribute and the function.
    fn documented() {}

    #[tokio::test]
    async fn of_tokio() {}

    #[testlog(tokio::test)]
    async fn wrapped() {}

    #[perf]
    fn registered_by_an_unknown_attribute() {}
}

#[cfg(test)]
mod declared;

#[cfg(not(test))]
mod real {
    pub fn only_in_production() {}
}

#[cfg(any(test, feature = "test-support"))]
pub fn for_tests_of_other_crates() {}
