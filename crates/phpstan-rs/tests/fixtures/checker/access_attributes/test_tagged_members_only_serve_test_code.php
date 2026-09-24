<?php
namespace Dave\Liddament\PhpLanguageExtensions\Attributes {
    #[\Attribute] final class Friend { public function __construct(string ...$classes) {} }
    #[\Attribute] final class NamespaceVisibility { public function __construct(?string $namespace = null, bool $excludeSubNamespaces = false) {} }
    #[\Attribute] final class Package {}
    #[\Attribute] final class TestTag {}
    #[\Attribute] final class RestrictTraitTo { public function __construct(string ...$classes) {} }
    #[\Attribute] final class Sealed { public function __construct(array $classes) {} }
    #[\Attribute] final class MustUseResult {}
    #[\Attribute] final class InjectableVersion {}
    #[\Attribute] final class CheckInjectableVersion {}
}
namespace App {
use Dave\Liddament\PhpLanguageExtensions\Attributes\TestTag;
final class Clock { #[TestTag] public function freeze(): void {} }
final class Service { public function f(Clock $c): void { $c->freeze(); } }
}
namespace App\Tests {
final class ClockTest { public function t(\App\Clock $c): void { $c->freeze(); } }
}