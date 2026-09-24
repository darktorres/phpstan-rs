<?php
            interface Shape3 {}
            /** @param interface-string $x */
            function f($x): void {}
            f(Shape3::class);
        