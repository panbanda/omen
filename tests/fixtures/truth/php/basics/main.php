<?php

declare(strict_types=1);

namespace Shop;

use ArrayObject;
use SplStack;
use Shop\Other;
use Shop\Shapes\Circle;
use Shop\Shapes\{Shape, Square, Unit};
use Shop\Util\Helpers as Tools;
use function Shop\Util\helper;

require_once __DIR__ . '/shapes.php';
require_once __DIR__ . '/util.php';
require_once __DIR__ . '/other.php';

function local(): int
{
    return 4;
}

function bare(): int
{
    return local();
}

function imported(): int
{
    return helper();
}

function namespaced(): int
{
    return Other\only_here();
}

function fully_qualified(): int
{
    return \Shop\Other\helper();
}

function aliased(): int
{
    return Tools::helper();
}

function qualified(): Circle
{
    return Circle::unit();
}

function construct(): float
{
    $circle = new Circle(1.0);
    return $circle->area();
}

function measure(Square $size): float
{
    return $size->area();
}

function unit_label(Unit $unit): string
{
    return $unit->label();
}

function typed(Shape $shape): float
{
    return $shape->area();
}

function dynamic($shape): float
{
    return $shape->area();
}

function outer(): int
{
    function inner(): int
    {
        return 5;
    }
    return inner();
}

function twice_local(int $value): int
{
    return $value * 2;
}

function callback(): array
{
    return array_map(function (int $n): int {
        return twice_local($n);
    }, [1, 2]);
}

function closure(): int
{
    $add = function (int $n): int {
        return $n + 1;
    };
    $double = fn(int $n): int => $n * 2;
    return $add(2) + $double(2);
}

function shadow(): int
{
    $helper = fn(): int => 9;
    return $helper();
}

function external(): string
{
    $stack = new SplStack();
    $stack->push(1);
    $list = new ArrayObject([1]);
    return strtoupper((string) (count($stack) + count($list)));
}

function main(): void
{
    $size = measure(new Square(2.0));
    $total = bare() + imported() + namespaced() + fully_qualified() + aliased();
    $circle = qualified();
    echo $total, ' ', construct(), ' ', $circle->diameter(), ' ', $circle->label(), ' ', $size, "\n";
    echo unit_label(Unit::Metre), ' ', typed($circle), ' ', dynamic(Circle::twin()), ' ', $circle->describe(), "\n";
    echo outer(), ' ', array_sum(callback()), ' ', closure(), ' ', shadow(), ' ', external(), "\n";
}

main();
