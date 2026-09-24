<?php
            class Foo { public int $bar = 0; public static int $baz = 0; }
            function test(Foo $f, array $x, string $name) {
                $a = 1;
                $f->bar = 1;
                $x['k'] = 1;
                $x[] = 1;
                Foo::$baz = 1;
                $f->{$name} = 1;
                [$a, $f->bar] = $x;
            }
        