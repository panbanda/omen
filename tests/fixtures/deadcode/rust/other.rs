use external::Foreign;

pub fn run() {
    let x = Foreign::new();
    x.dead_unresolved();
}
