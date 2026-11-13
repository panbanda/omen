<?php

declare(strict_types=1);

namespace Shop\Shapes;

use function Shop\Util\twice;

require_once __DIR__ . '/util.php';

interface Shape
{
    public function area(): float;
}

trait Describes
{
    public function describe(): string
    {
        return 'shape ' . $this->area();
    }
}

enum Unit: string
{
    case Metre = 'm';
    case Foot = 'ft';

    public function label(): string
    {
        return strtoupper($this->value);
    }
}

abstract class Base implements Shape
{
    use Describes;

    public function __construct(protected string $name)
    {
    }

    public function label(): string
    {
        return $this->name;
    }
}

class Circle extends Base
{
    public function __construct(private float $radius)
    {
        parent::__construct('circle');
    }

    public static function unit(): self
    {
        return self::build(1.0);
    }

    public static function twin(): self
    {
        return static::unit();
    }

    private static function build(float $radius): self
    {
        return new self($radius);
    }

    public function area(): float
    {
        return $this->scaled(3.14);
    }

    public function diameter(): float
    {
        return $this->radius() * 2;
    }

    public function label(): string
    {
        return strtoupper(parent::label());
    }

    protected function radius(): float
    {
        return $this->radius;
    }

    private function scaled(float $factor): float
    {
        return $factor * $this->radius * $this->radius * twice(1);
    }
}

class Square extends Base
{
    public function __construct(private float $side)
    {
        parent::__construct('square');
    }

    public function area(): float
    {
        return $this->side * $this->side;
    }
}

class Pair
{
    private Circle $lead;
    private ?Pair $next = null;
    private $spare;

    public function __construct(Circle $lead, private Square $corner)
    {
        $this->lead = $lead;
        $this->spare = new Circle(2.0);
    }

    public function leadArea(): float
    {
        return $this->lead->area();
    }

    public function nextArea(): float
    {
        return $this->next->lead->area();
    }

    public function cornerArea(): float
    {
        return $this->corner->area();
    }

    public function spareArea(): float
    {
        return $this->spare->area();
    }
}
