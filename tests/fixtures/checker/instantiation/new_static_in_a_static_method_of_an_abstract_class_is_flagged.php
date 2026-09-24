<?php
            abstract class AbsFactory {
                final public function __construct() {}
                public static function make(): static { return new static(); }
                public function copy(): static { return new static(); }
            }
        