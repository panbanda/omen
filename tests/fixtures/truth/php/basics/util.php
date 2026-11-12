<?php

declare(strict_types=1);

namespace Shop\Util;

function helper(): int
{
    return 1;
}

function twice(int $value): int
{
    return $value * 2;
}

final class Helpers
{
    public static function helper(): int
    {
        return 5;
    }
}
