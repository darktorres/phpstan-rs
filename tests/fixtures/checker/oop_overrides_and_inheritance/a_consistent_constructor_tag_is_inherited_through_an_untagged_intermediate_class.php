<?php
            /** @phpstan-consistent-constructor */
            class Base14 { public function __construct(string $a) {} }
            class Middle14 extends Base14 {}
            class Sub14 extends Middle14 { public function __construct() {} }
        