<?php
            class Banned {}
            function f(object $x): bool { return $x instanceof Banned; }
        