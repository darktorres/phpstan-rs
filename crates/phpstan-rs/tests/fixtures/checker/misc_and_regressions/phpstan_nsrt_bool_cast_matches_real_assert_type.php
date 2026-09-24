<?php
            use function PHPStan\Testing\assertType;
            function test(\SimpleXMLElement $xml): void {
                assertType('bool', (bool) $xml->Exists);
            }
        