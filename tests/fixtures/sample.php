<?php

use App\Models\User;
use App\Services\Logger;

class UserRepository {
    private $db;
    private $logger;

    public function find($id) {
        return $this->db->find($id);
    }

    public function save($user) {
        return $this->db->save($user);
    }
}

function format_name($first, $last) {
    return trim($first . ' ' . $last);
}

function calculate_tax($amount, $rate) {
    return $amount * $rate;
}
