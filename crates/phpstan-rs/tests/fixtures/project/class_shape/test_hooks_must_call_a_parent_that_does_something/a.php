<?php
namespace PHPUnit\Framework { abstract class TestCase {
    protected function setUp(): void {}
    protected function tearDown(): void {}
} }
namespace App\Tests {
abstract class BaseTest extends \PHPUnit\Framework\TestCase {
    protected function setUp(): void { $this->boot(); }
    protected function tearDown(): void { $this->boot(); }
    protected function boot(): void {}
}
final class MissingTest extends BaseTest {
    protected function setUp(): void {
        $this->x = 1;
    }
    protected function tearDown(): void {
        $this->x = 2;
    }
}
final class LateTest extends BaseTest {
    protected function setUp(): void {
        $this->x = 1;
        parent::setUp();
    }
}
final class GoodTest extends BaseTest {
    protected function setUp(): void {
        parent::setUp();
        $this->x = 1;
    }
}
final class DirectTest extends \PHPUnit\Framework\TestCase {
    protected function setUp(): void { $this->x = 1; }
}
}
