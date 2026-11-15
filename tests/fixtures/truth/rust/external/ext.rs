pub trait Shout {
    fn shout(&self) -> String;
}

impl Shout for String {
    fn shout(&self) -> String {
        self.to_uppercase()
    }
}

pub fn drop<T>(_value: T) {}
