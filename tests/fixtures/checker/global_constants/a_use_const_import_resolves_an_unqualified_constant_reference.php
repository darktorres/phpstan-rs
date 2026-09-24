<?php
            namespace Bar {
                const ZED = 5;
                function zed(): int { return 1; }
            }
            namespace Foo {
                use const Bar\ZED;
                use function Bar\zed;
                use function PHPStan\Testing\assertType;
                function run(): void {
                    assertType('5', ZED);
                    assertType('int', zed());
                }
            }
        