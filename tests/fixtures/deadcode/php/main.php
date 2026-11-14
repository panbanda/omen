<?php
require_once "other.php";

class Tool
{
    public function run(): void
    {
        $this->liveDirect();
        array_map([$this, 'liveValue'], [1]);
    }

    public function livePublic(): void {}

    private function liveDirect(): void {}

    private function liveValue(): void {}

    private function deadPlain(): void {}

    private function deadCaller(): void
    {
        $this->deadCallee();
    }

    private function deadCallee(): void {}

    private function deadUnresolved(): void {}
}

(new Tool())->run();
runOther(null);
