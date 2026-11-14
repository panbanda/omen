<?php
require_once 'util.php';

use Vendor\Pkg\Tool;

function helper() { return 1; }

function run($text) {
    strlen($text);
    array_map('trim', [$text]);
    Tool::make();
    helper();
}

function decoy($text) {
    return str_replace("a", "b", $text);
}

function parameter($count) {
    return $count(1);
}
