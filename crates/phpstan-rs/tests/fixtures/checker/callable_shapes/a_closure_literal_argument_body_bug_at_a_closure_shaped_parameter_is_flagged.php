<?php
            class Foo {}
            /** @param Closure(Foo): void $fn */
            function apply($fn): void {}
            apply(function ($x) {
                $x->paint();
            });
        