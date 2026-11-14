pub fn only_here() -> u32 {
    1
}

pub mod checks {
    pub fn only_here() -> u32 {
        2
    }
}

pub struct Circle {
    radius: f64,
}

pub struct Square {
    side: f64,
}

impl Circle {
    pub fn area(&self) -> f64 {
        3.14 * self.radius * self.radius
    }
}

impl Square {
    pub fn area(&self) -> f64 {
        self.side * self.side
    }
}
