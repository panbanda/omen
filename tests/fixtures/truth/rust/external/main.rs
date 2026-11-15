mod ext;

use ext::drop;
use ext::Shout;
use std::collections::HashMap;

fn maps() {
    let mut names: HashMap<String, i32> = HashMap::new();
    names.insert("a".to_string(), 1);
}

fn lists() {
    let list: Vec<i32> = Vec::new();
    list.len();
}

fn texts(text: String) -> String {
    text.shout()
}

fn crates() {
    let _parsed = serde_json::from_str::<i32>("1");
    let _text = std::fs::read_to_string("x");
}

fn prelude() -> Option<i32> {
    let value: Result<i32, i32> = Err(1);
    let _ = value;
    Some(3)
}

fn decoy(list: Vec<i32>) {
    drop(list);
}

fn main() {
    maps();
    lists();
    let _ = texts(String::new());
    crates();
    let _ = prelude();
    decoy(Vec::new());
}
