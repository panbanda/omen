mod other;

struct Tool;

impl Tool {
    fn live_method(&self) {}
    fn dead_method(&self) {}
}

fn live_direct() {}
fn live_value() {}
fn dead_plain() {}
fn dead_caller() {
    dead_callee();
}
fn dead_callee() {}
fn dead_unresolved() {}

pub fn live_public() {}

fn main() {
    live_direct();
    let handler: fn() = live_value;
    handler();
    let tool: Tool = Tool;
    tool.live_method();
    other::run();
}
