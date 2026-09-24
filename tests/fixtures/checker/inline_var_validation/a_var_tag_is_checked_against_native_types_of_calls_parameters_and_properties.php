<?php
            class Foo {}
            class Bar {}
            class C {
                public Foo $foo;
                public function make(): Foo { return new Foo(); }
                public function f(int $n): void {
                    /** @var string $a */
                    $a = $n;
                    /** @var Bar $b */
                    $b = $this->make();
                    /** @var Bar $c */
                    $c = $this->foo;
                    /** @var list<int> $d */
                    $d = g();
                    /** @var string $e */
                    $e = g();
                }
            }
            function g(): array { return []; }
        