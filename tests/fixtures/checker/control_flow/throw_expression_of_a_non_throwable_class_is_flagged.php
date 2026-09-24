<?php
            class NotAnException3 {}
            function test(?int $x): int {
                return $x ?? throw new NotAnException3();
            }
        