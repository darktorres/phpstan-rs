<?php
            class TagId18 {}
            class OtherId18 {}
            /** @param array<TagId18|OtherId18|null> $x
             *  @return array<TagId18>
             */
            function f(array $x): array {
                return array_filter($x, static fn (mixed $v): bool => $v instanceof TagId18);
            }
        