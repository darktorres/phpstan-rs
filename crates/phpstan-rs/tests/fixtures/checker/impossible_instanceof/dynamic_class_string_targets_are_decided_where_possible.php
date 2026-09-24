<?php
            class Base {}
            class Child extends Base {}
            final class Sealed {}
            class Other {}
            /**
             * @param class-string<Base> $base
             * @param class-string<Sealed> $sealed
             */
            function f(Other $o, Child $c, Sealed $s, string $base, string $sealed): void {
                $lit = 'Other';
                if ($o instanceof $lit) { echo 1; }
                if ($o instanceof $base) { echo 2; }
                if ($c instanceof $base) { echo 3; }
                if ($s instanceof $sealed) { echo 4; }
            }
        