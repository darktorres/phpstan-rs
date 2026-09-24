<?php
            class Foo {
                public function doFoo(\Exception $e): void {}
            }
            class Lorem extends Foo {
                public function doFoo(\InvalidArgumentException $e): void {}
            }
        