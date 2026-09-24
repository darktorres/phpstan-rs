<?php
            class Foo {}
            /** @param int[] $xs */
            function test(array $xs, Foo $f): void {
                $xs + $f;
            }
        