<?php
            final class Foo {}
            /**
             * @return 'Bar'
             */
            function f(Foo $x) {
                return get_debug_type($x);
            }
        