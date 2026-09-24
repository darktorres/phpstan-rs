<?php
            interface Foo { public function doFoo(); }
            abstract class Bar implements Foo {
                public function doBar() {}
                abstract public function doBaz();
            }
            class Baz extends Bar {
                public function doBar() {}
            }
        