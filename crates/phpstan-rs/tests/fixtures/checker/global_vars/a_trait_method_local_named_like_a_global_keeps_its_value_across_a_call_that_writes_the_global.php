<?php
            use function PHPStan\Testing\assertType;
            function setIt(): void {
                global $x;
                $x = null;
            }
            trait T {
                public function m(): void {
                    $x = 1;
                    setIt();
                    assertType('1', $x);
                }
            }
