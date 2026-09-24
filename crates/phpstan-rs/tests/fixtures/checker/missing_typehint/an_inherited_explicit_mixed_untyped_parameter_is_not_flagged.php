<?php
            interface LoggerLike {
                /**
                 * @param mixed $level
                 */
                public function log($level, string $message): void;
            }
            class SpyLogger implements LoggerLike {
                public function log($level, string $message): void {}
            }
        