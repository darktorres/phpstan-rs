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
namespace App\Domain {
use Dave\Liddament\PhpLanguageExtensions\Attributes\NamespaceVisibility;
use Dave\Liddament\PhpLanguageExtensions\Attributes\Package;
#[NamespaceVisibility]
final class Wide { public function run(): void {} }
final class Narrow { #[Package] public function run(): void {} }
}
namespace App\Domain\Sub {
final class Inner { public function f(\App\Domain\Wide $w, \App\Domain\Narrow $n): void { $w->run(); $n->run(); } }
}
namespace App\Ui {
final class Outer { public function f(\App\Domain\Wide $w): void { $w->run(); } }
}