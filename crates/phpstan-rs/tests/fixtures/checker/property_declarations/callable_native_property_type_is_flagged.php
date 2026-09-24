<?php
            class Holder {
                public callable $a;
                public ?callable $b;
                public \Closure|callable $c;
                public \Closure $ok;
                public function __construct(private callable $d) {}
            }
        