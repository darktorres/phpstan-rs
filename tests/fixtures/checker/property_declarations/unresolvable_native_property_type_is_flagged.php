<?php
            final class Foo {}
            final class Bar {}
            class Impossible {
                public Foo&Bar $a;
                public \Countable&\Iterator $ok;
                public function __construct(public Bar&Foo $b) {}
            }
        