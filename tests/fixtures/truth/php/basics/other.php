<?php

declare(strict_types=1);

namespace Shop\Other;

function helper(): int
{
    return 2;
}

function only_here(): int
{
    return 3;
}

function push(int $value): int
{
    return $value;
}
