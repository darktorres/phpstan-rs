<?php
            class NotFinal {}
            /**
             * @return 'NotFinal'
             */
            function f(NotFinal $x) {
                return get_debug_type($x);
            }
        