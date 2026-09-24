<?php
            abstract class Shape {
                public static function make(): static { return new static(); }
                public static function bad(): self { return new self(); }
            }
        