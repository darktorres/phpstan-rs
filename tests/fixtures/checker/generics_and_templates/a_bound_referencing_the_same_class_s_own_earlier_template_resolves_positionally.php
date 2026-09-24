<?php
            interface Shape {}
            class Circle implements Shape {}
            class NotAShape {}
            /**
             * @template A of Shape
             * @template B of A
             */
            class Pair {}
            /** @param Pair<Circle, NotAShape> $p */
            function f($p) {}
        