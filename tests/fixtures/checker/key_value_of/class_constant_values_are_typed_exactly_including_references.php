<?php
            use function PHPStan\Testing\assertType;
            enum Level: int {
                case Debug = 100;
                case Info = 200;
                const VALUES = [100, 200];
                const NAMES = ['debug' => self::Debug, 'info' => self::Info];
            }
            class Other { const C = 'c'; }
            class Consts {
                const A = self::B;
                const B = Other::C;
                const LIST = [self::A, 'x', 7 => true, false];
                const EMPTY = [];
                const LOOP_A = self::LOOP_B;
                const LOOP_B = self::LOOP_A;
            }
            /** @param value-of<Level::VALUES> $level */
            function f(int $level): void { assertType('100|200', $level); }
            function g(): void {
                assertType("'c'", Consts::A);
                assertType("array{0: 'c', 1: 'x', 7: true, 8: false}", Consts::LIST);
                assertType('array{}', Consts::EMPTY);
                assertType("array{debug: Level::Debug, info: Level::Info}", Level::NAMES);
                assertType('mixed', Consts::LOOP_A);
            }
        