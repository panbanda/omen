use std::rc::Rc;
use std::sync::Arc;

mod updater;

pub fn run() -> usize {
    helper::update()
}

mod helper {
    use super::updater::perform_update;

    pub fn update() -> usize {
        perform_update()
    }
}

pub struct Engine;

impl Engine {
    pub fn compute(&self) -> usize {
        1
    }

    pub fn len(&self) -> usize {
        2
    }

    pub fn start(self) -> usize {
        let this = Arc::new(self);
        this.compute()
    }

    pub fn start_local(self) -> usize {
        let this = Rc::new(self);
        this.compute()
    }

    pub fn share(self: &Arc<Self>) -> usize {
        let other = self.clone();
        other.compute()
    }

    pub fn share_again(self: &Arc<Self>) -> usize {
        let again = Arc::clone(self);
        again.compute()
    }

    pub fn keep(&self) -> usize {
        let me = self;
        me.compute()
    }

    pub fn wrap_text(&self, text: String) -> usize {
        let this = Arc::new(text);
        this.len()
    }
}
