<?php

namespace App;

class Service
{
    public function production(): int
    {
        return 1;
    }

    /** Runs the test of the connection. */
    public function testConnection(): int
    {
        return $this->production();
    }
}
