<?php
            use function PHPStan\Testing\assertType;
            class Level {
                /** @var list<100|200> */
                const VALUES = [100, 200];
            }
            /** @param value-of<Level::VALUES> $level */
            function f(int $level): void { assertType('100|200', $level); }
        