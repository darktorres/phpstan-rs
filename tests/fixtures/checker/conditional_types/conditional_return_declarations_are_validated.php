<?php
            /**
             * @return ($missing is int ? int : string)
             */
            function a(int $x): int|string { return 1; }
            /**
             * @return (Foo is int ? int : string)
             */
            function b(int $x): int|string { return 1; }
            /**
             * @return ($x is int ? int : string)
             */
            function c(int $x): int|string { return 1; }
            /**
             * @return ($x is not string ? int : string)
             */
            function d(int $x): int|string { return 1; }
            /**
             * @return ($x is int ? int : string)
             */
            function e(int|string $x): int|string { return 1; }
        