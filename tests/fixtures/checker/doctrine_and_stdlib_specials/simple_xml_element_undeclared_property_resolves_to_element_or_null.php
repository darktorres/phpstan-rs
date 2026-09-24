<?php
            use function PHPStan\Testing\assertType;
            function f(\SimpleXMLElement $xml): void {
                assertType('SimpleXMLElement|null', $xml->foo);
            }
        