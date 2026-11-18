<?php
class JobController {
    public function create($value) {
        $load = function () use ($value) {
            system($value);
        };
        dispatch($load);
        dispatch([$this, 'audit']);
        dispatch(notify(...));
        return 'ok';
    }

    public function shadowed($audit) {
        dispatch($audit);
        return 'ok';
    }

    public function otherFile() {
        dispatch([Library::class, 'cleanup']);
        return 'ok';
    }

    public function textOnly() {
        // dispatch([$this, 'audit']);
        dispatch('audit');
        return 'audit';
    }

    private function audit() {
        system('logger ' . getenv('EVENT'));
    }
}

function notify() {
    system('notify ' . getenv('EVENT'));
}
