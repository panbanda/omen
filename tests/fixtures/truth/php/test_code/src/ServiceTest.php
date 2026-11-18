<?php

namespace App;

use PHPUnit\Framework\Attributes\Test;
use PHPUnit\Framework\TestCase;

class ServiceTest extends TestCase
{
    private Service $service;

    protected function setUp(): void
    {
        $this->service = new Service();
    }

    public function testProduction(): void
    {
        $this->assertSame(1, $this->value());
    }

    /**
     * @test
     */
    public function it_gives_one(): void
    {
        $this->assertSame(1, $this->value());
    }

    #[Test]
    public function with_an_attribute(): void
    {
        $this->assertSame(1, $this->value());
    }

    private function value(): int
    {
        return $this->service->production();
    }
}
