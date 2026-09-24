<?php
            namespace Bar {
                function zed(): int { return 1; }
            }
            namespace Foo {
                use function Bar\zed;
                function run(): void {
                    echo zed;
                }
            }
        