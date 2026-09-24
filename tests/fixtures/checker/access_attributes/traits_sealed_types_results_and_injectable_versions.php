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
use Dave\Liddament\PhpLanguageExtensions\Attributes\{RestrictTraitTo, Sealed, MustUseResult, InjectableVersion, CheckInjectableVersion};
abstract class Controller {}
#[RestrictTraitTo(Controller::class)]
trait Renders {}
final class HomeController extends Controller { use Renders; }
final class Mailer { use Renders; }
#[Sealed([Circle::class])]
abstract class Shape {}
final class Circle extends Shape {}
final class Square extends Shape {}
final class Calc { #[MustUseResult] public function add(int $a): int { return $a; } }
#[InjectableVersion]
interface Repo {}
final class DbRepo implements Repo {}
final class UsesRepo {
    public function __construct(DbRepo $repo, Repo $ok) {}
    #[CheckInjectableVersion]
    public function set(DbRepo $r): void {}
    public function unchecked(DbRepo $r): void {}
}
function run(Calc $c): void { $c->add(1); }
}