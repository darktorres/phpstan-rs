<?php
            class Real {
                const FOO = 1;
                /** @see self::FOO */
                public function bar(): void {}
                /** @see self::bar() */
                public function baz(): void {}
            }
