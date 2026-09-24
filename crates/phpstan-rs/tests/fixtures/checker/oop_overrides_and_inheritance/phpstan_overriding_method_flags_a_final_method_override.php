<?php
            class Foo {
                final public function doFoo(): void {}
            }
            class Bar extends Foo {
                public function doFoo(): void {}
            }
        