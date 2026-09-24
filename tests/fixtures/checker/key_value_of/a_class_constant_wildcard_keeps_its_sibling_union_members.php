<?php
            use function PHPStan\Testing\assertType;
            enum Level: int { case Debug = 100; case Info = 200; }
            class LogLevel { const DEBUG = 'debug'; const INFO = 'info'; const OTHER = 1; }
            /** @phpstan-param Level|LogLevel::* $level */
            function b(int|string|Level $level): void {
                assertType("Level|'debug'|'info'|1", $level);
            }
            /** @param LogLevel::D* $level */
            function c($level): void { assertType("'debug'", $level); }
            /** @param Level::Debug|LogLevel::INFO $level */
            function d($level): void { assertType("Level::Debug|'info'", $level); }
            class Self_ {
                const RFC_A = 'a';
                const RFC_B = 'b';
                /** @param self::RFC_* $rfc */
                public function e($rfc): void { assertType("'a'|'b'", $rfc); }
            }
        