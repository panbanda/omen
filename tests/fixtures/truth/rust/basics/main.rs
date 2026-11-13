mod other;
mod shapes;
mod util;

use std::collections::HashMap;
use shapes::{Circle, Shape, Square};
use util::helper;
use util::twice as double;
use other::*;
pub(crate) use shapes::describe;
use crate::shapes::Unit;

fn local() -> u32 {
    4
}

fn run() -> u32 {
    fn inner() -> u32 {
        5
    }
    let total = local() + helper() + double(2) + inner() + only_here();
    let circle = Circle::new(1.0);
    let _ = circle.area();
    let numbers: Vec<u32> = vec![1, 2].into_iter().map(|n| twice_local(n)).collect();
    let add = |n: u32| n + 1;
    total + add(numbers.len() as u32)
}

fn twice_local(value: u32) -> u32 {
    value * 2
}

fn measure(size: Square) -> f64 {
    size.area()
}

fn shadow() -> u32 {
    let helper = || 9;
    helper()
}

fn dynamic(shape: &dyn Shape, unit: Unit) -> f64 {
    let _ = unit;
    shape.area()
}

fn external() -> usize {
    let mut seen = HashMap::new();
    seen.insert(1, 2);
    seen.len()
}

fn main() {
    let size = measure(Square::new(2.0));
    println!("{} {} {} {}", run(), shadow(), describe(), size);
    let _ = (dynamic(&Circle { radius: 1.0 }, Unit::Metre), external());
}
