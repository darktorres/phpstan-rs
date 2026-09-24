<?php
            use function PHPStan\Testing\assertType;
            enum ConnectedWith {
                case ApiKey;
                case Password;
            }
            function f(ConnectedWith $valid_key): void {
                $secret_key = null;
                if ($valid_key === ConnectedWith::ApiKey) {
                    $secret_key = "abc";
                }
                echo "unrelated code in between";
                if ($valid_key === ConnectedWith::ApiKey) {
                    assertType("'abc'", $secret_key);
                }
            }
        