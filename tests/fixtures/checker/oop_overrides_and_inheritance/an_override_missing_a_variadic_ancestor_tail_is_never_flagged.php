<?php
            class Base { public function foo(int $a, ...$rest): void {} }
            class Child extends Base { public function foo(int $a): void {} }
        