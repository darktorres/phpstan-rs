<?php
            use function PHPStan\Testing\assertType;
            class MyXml extends \SimpleXMLElement {}
            function f(MyXml $xml): void {
                assertType('MyXml|null', $xml->foo);
            }
        