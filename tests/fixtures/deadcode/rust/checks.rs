#[ctor]
fn registered_at_load() {
    live_from_ctor();
}

fn live_from_ctor() {}

#[no_mangle]
fn live_exported_symbol() {}

extern "C" fn live_foreign_callback(_value: u32) {}

#[allow(dead_code)]
fn live_kept_by_the_author() {}

fn test_like_name_in_production() {}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod checks {
    fn live_helper_of_a_test() {}

    fn dead_helper_in_test_code() {}

    #[test]
    /// The documentation is between the attribute and the function.
    fn documented_check() {
        live_helper_of_a_test();
    }

    #[testlog(tokio::test)]
    async fn wrapped_check() {}
}
