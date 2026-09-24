<?php
            /**
             * @return 'integer'|'string'
             */
            function f(int|string $x) {
                return gettype($x);
            }
        