<?php
            class Transformation {}
            class Foo {
                /** @return Transformation[] */
                public function f(): iterable {
                    yield new Transformation();
                }
            }
        