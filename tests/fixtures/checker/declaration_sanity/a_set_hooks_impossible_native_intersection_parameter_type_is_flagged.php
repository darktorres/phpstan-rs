<?php
            final class Foo {}
            final class Bar {}
            class Baz {
                public mixed $bar {
                    set(Foo&Bar $value) {
                        $this->bar = $value;
                    }
                }
            }
        