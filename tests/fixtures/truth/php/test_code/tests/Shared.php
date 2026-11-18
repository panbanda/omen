<?php

namespace App\Tests;

use App\Service;

class Shared
{
    public static function build(): Service
    {
        return new Service();
    }
}
