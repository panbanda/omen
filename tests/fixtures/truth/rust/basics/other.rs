pub fn helper() -> u32 {
    2
}

pub fn only_here() -> u32 {
    3
}

pub fn insert(value: u32) -> u32 {
    value
}

mod checks {
    fn only_here() -> u32 {
        9
    }

    fn verify() -> u32 {
        only_here()
    }
}

pub fn qualified() -> u32 {
    crate::util::helper() + super::util::twice(1)
}
