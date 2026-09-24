<?php
            class Base {}
            class Child extends Base {
                public function __construct(#[\Override] public int $x = 1) {}
            }
        