use tools::command::new_command;

fn imported() -> usize {
    new_command()
}

fn qualified() -> usize {
    tools::command::new_command() + tools::init()
}

fn renamed() -> usize {
    kit::draw() + kit::init()
}

fn registry() -> usize {
    num_cpus::get()
}

fn main() {
    let total = app::run() + imported() + qualified() + renamed() + registry();
    println!("{total}");
}
