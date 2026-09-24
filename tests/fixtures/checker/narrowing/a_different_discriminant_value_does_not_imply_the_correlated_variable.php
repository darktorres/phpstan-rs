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
                if ($valid_key === ConnectedWith::Password) {
                    // Nothing narrows $secret_key here - the recorded
                    // fact is keyed to the ApiKey discriminant
                    // specifically, not a general "valid_key was
                    // compared" signal - so it stays whatever the first
                    // if's own ordinary merge already produced.
                    assertType("'abc'|null", $secret_key);
                }
            }
        