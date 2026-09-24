<?php
            use function PHPStan\Testing\assertType;
            class Connection {}
            function f(bool $shouldLock): void {
                $conn = null;
                $name = null;
                if ($shouldLock) {
                    $conn = new Connection();
                    $name = "lockname";
                }
                echo "unrelated code in between";
                if ($conn instanceof Connection) {
                    assertType("'lockname'", $name);
                }
            }
        