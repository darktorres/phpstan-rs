<?php
            class BaseA { public static function foo(): void {} }
            class ChildA extends BaseA { public static function foo(): void {} }
            class BaseB { public function bar(): void {} }
            class ChildB extends BaseB { public function bar(): void {} }
        