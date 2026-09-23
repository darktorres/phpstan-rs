//! `php-language-extensions` access attributes.

use super::helpers::*;

const PREFIX: &str = "<?php
namespace Dave\\Liddament\\PhpLanguageExtensions\\Attributes {
    #[\\Attribute] final class Friend { public function __construct(string ...$classes) {} }
    #[\\Attribute] final class NamespaceVisibility { public function __construct(?string $namespace = null, bool $excludeSubNamespaces = false) {} }
    #[\\Attribute] final class Package {}
    #[\\Attribute] final class TestTag {}
    #[\\Attribute] final class RestrictTraitTo { public function __construct(string ...$classes) {} }
    #[\\Attribute] final class Sealed { public function __construct(array $classes) {} }
    #[\\Attribute] final class MustUseResult {}
    #[\\Attribute] final class InjectableVersion {}
    #[\\Attribute] final class CheckInjectableVersion {}
}
";

fn ids(body: &str) -> Vec<String> {
    let src = format!("{PREFIX}{body}");
    let mut v: Vec<String> = diagnostics_for(&src)
        .into_iter()
        .filter(|d| {
            d.identifier.starts_with("attribute.")
                || d.identifier.ends_with(".resultDiscarded")
                || d.identifier.ends_with("disallowedSubtype")
        })
        .map(|d| format!("{}: {}", d.identifier, d.message))
        .collect();
    v.sort();
    v
}

#[test]
fn friends_are_the_only_other_callers() {
    let found = ids("namespace App {
use Dave\\Liddament\\PhpLanguageExtensions\\Attributes\\Friend;
final class Person {
    #[Friend(PersonBuilder::class)]
    public function __construct() {}
    #[Friend(PersonBuilder::class)]
    public function rename(): void { $this->rename(); }
}
final class PersonBuilder { public function build(): Person { $p = new Person(); $p->rename(); return $p; } }
final class Stranger { public function make(): void { $p = new Person(); $p->rename(); } }
}");
    assert_eq!(
        found,
        vec![
            "attribute.friend: App\\Person::__construct() can only be called by its friends (App\\PersonBuilder), not by App\\Stranger.",
            "attribute.friend: App\\Person::rename() can only be called by its friends (App\\PersonBuilder), not by App\\Stranger.",
        ]
    );
}

#[test]
fn namespace_visibility_and_package_limit_callers_by_namespace() {
    let found = ids("namespace App\\Domain {
use Dave\\Liddament\\PhpLanguageExtensions\\Attributes\\NamespaceVisibility;
use Dave\\Liddament\\PhpLanguageExtensions\\Attributes\\Package;
#[NamespaceVisibility]
final class Wide { public function run(): void {} }
final class Narrow { #[Package] public function run(): void {} }
}
namespace App\\Domain\\Sub {
final class Inner { public function f(\\App\\Domain\\Wide $w, \\App\\Domain\\Narrow $n): void { $w->run(); $n->run(); } }
}
namespace App\\Ui {
final class Outer { public function f(\\App\\Domain\\Wide $w): void { $w->run(); } }
}");
    assert_eq!(
        found,
        vec![
            "attribute.namespaceVisibility: App\\Domain\\Narrow::run() is only visible from namespace App\\Domain.",
            "attribute.namespaceVisibility: App\\Domain\\Wide::run() is only visible from namespace App\\Domain and below.",
        ]
    );
}

#[test]
fn test_tagged_members_only_serve_test_code() {
    let found = ids("namespace App {
use Dave\\Liddament\\PhpLanguageExtensions\\Attributes\\TestTag;
final class Clock { #[TestTag] public function freeze(): void {} }
final class Service { public function f(Clock $c): void { $c->freeze(); } }
}
namespace App\\Tests {
final class ClockTest { public function t(\\App\\Clock $c): void { $c->freeze(); } }
}");
    assert_eq!(
        found,
        vec![
            "attribute.testTag: App\\Clock::freeze() is marked #[TestTag] and can only be used from test code."
        ]
    );
}

#[test]
fn traits_sealed_types_results_and_injectable_versions() {
    let found = ids("namespace App {
use Dave\\Liddament\\PhpLanguageExtensions\\Attributes\\{RestrictTraitTo, Sealed, MustUseResult, InjectableVersion, CheckInjectableVersion};
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
}");
    assert_eq!(
        found,
        vec![
            "attribute.injectableVersion: Parameter $r of App\\UsesRepo::set() is type-hinted with App\\DbRepo; inject its #[InjectableVersion] App\\Repo instead.",
            "attribute.injectableVersion: Parameter $repo of App\\UsesRepo::__construct() is type-hinted with App\\DbRepo; inject its #[InjectableVersion] App\\Repo instead.",
            "attribute.restrictTraitTo: Trait App\\Renders can only be used by App\\Controller and their subclasses, not App\\Mailer.",
            "class.disallowedSubtype: Type App\\Square is not allowed to be a subtype of App\\Shape.",
            "method.resultDiscarded: Call to App\\Calc::add() on a separate line discards its return value.",
        ]
    );
}
