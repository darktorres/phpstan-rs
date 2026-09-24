<?php
            interface Shape {}
            class Circle {}
            class Square {}
            final class Locked {}
            enum Suit { case Hearts; }
            trait Mixin {}
            function f(int $i, Circle $c, Locked $l, Suit $s): void {
                if ($i instanceof Circle) { echo 1; }
                if ($c instanceof Square) { echo 2; }
                if ($l instanceof Shape) { echo 3; }
                if ($s instanceof Shape) { echo 4; }
                if ($c instanceof Mixin) { echo 5; }
            }
        