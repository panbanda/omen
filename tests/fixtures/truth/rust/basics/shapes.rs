use super::util::twice;

pub struct Circle {
    pub radius: f64,
}

pub struct Square {
    pub side: f64,
}

pub trait Shape {
    fn area(&self) -> f64;
}

pub enum Unit {
    Metre,
    Foot,
}

pub type Scale = f64;

impl Circle {
    pub fn new(radius: f64) -> Self {
        Circle { radius }
    }

    pub fn area(&self) -> f64 {
        self.scaled(3.14)
    }

    fn scaled(&self, factor: Scale) -> f64 {
        factor * self.radius * self.radius * twice(1) as f64
    }
}

impl Square {
    pub fn new(side: f64) -> Self {
        Square { side }
    }

    pub fn area(&self) -> f64 {
        self.side * self.side
    }
}

impl Shape for Circle {
    fn area(&self) -> f64 {
        Circle::area(self)
    }
}

pub(crate) fn describe() -> &'static str {
    "shape"
}

fn hidden() -> u32 {
    7
}
