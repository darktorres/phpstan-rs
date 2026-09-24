<?php
            class Foo {}
            /** @param list<Foo> $x */
            function f(array $x): array {
                return array_filter($x);
            }
        