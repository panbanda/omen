<?php
class RunController {
    public function run($value) {
        return $this->prepare($value);
    }

    private function prepare($value) {
        return $this->execute($value);
    }

    private function execute($value) {
        system($value);
        return $value;
    }

    private function orphan($value) {
        system($value);
    }
}
