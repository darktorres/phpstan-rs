<?php
            final class Foo {}
            /**
             * @return 'Foo'
             */
            function f(Foo $x) {
                return get_debug_type($x);
            }
        