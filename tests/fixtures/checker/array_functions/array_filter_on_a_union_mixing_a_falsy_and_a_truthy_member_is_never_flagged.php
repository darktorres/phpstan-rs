<?php
            class Foo {}
            /** @param list<Foo|null> $x */
            function f(array $x): array {
                return array_filter($x);
            }
        