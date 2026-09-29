<?php
namespace Mockery { interface MockInterface {} class Mockery { public static function mock(mixed ...$args): mixed { return null; } } }
namespace { class Mockery extends \Mockery\Mockery {} }
namespace App {
use function PHPStan\Testing\assertType;
final class Mailer {}
final class Locator { public function get(string $id): mixed { return null; } }
function f(Locator $l): void {
    assertType('App\\Mailer&Mockery\\MockInterface', \Mockery::mock(Mailer::class));
    assertType('App\\Mailer', $l->get(Mailer::class));
    assertType('mixed', $l->get('nope'));
}
}