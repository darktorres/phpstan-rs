<?php
            use function PHPStan\Testing\assertType;
            class MyXml extends \SimpleXMLElement {
                public string $foo;
            }
            function f(MyXml $xml): void {
                assertType('string', $xml->foo);
            }
        