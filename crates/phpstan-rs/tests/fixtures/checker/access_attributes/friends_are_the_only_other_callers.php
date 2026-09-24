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
use Dave\Liddament\PhpLanguageExtensions\Attributes\Friend;
final class Person {
    #[Friend(PersonBuilder::class)]
    public function __construct() {}
    #[Friend(PersonBuilder::class)]
    public function rename(): void { $this->rename(); }
}
final class PersonBuilder { public function build(): Person { $p = new Person(); $p->rename(); return $p; } }
final class Stranger { public function make(): void { $p = new Person(); $p->rename(); } }
}