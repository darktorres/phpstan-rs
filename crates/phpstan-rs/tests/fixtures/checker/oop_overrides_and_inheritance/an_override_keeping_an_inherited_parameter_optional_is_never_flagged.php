<?php
            class Base { public function foo(int $a, int $b = 0): void {} }
            class Child extends Base { public function foo(int $a, int $b = 1): void {} }
        