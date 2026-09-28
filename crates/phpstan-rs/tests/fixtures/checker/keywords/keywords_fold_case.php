<?PHP
DECLARE(STRICT_TYPES=1);
trait A { public function hello(): string { return "a"; } }
trait B { public function hello(): string { return "b"; } }
class C {
    VAR $x = 1;
    USE A, B { B::hello INSTEADOF A; }
    public function __construct(PRIVATE(SET) string $name = "") {}
}
function f(int $i): void {}
f("1");
