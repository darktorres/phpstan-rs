<?php
        class Box { const A = 1; }
        function f(string $s, int $i, int|string $m): void {
            echo Box::{$s};
            echo Box::{$i};
            echo Box::{$m};
            echo Box::{$undefinedName};
        }
    