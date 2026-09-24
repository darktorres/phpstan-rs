<?php
            use function PHPStan\Testing\assertType;
            enum ConnectedWith {
                case AuthKey;
                case ApiKey;
            }
            function f(bool $isApiKey): void {
                $validKey = null;
                $secretKey = null;
                if ($isApiKey) {
                    $validKey = ConnectedWith::ApiKey;
                    $secretKey = "s3cr3t";
                }
                echo "unrelated code in between";
                if ($validKey === ConnectedWith::ApiKey) {
                    assertType("'s3cr3t'", $secretKey);
                }
            }
        