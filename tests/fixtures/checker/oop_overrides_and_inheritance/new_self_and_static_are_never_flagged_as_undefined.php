<?php
            class Foo {
                public static function make(): self { return new self(); }
                public static function makeStatic(): static { return new static(); }
            }
        