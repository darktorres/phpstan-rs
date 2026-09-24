<?php
            final class Logger {
                public const int EMERGENCY = 0;
                public const int OFF = -1;
                /** @param array{severity?: int|string} $options */
                public function __construct(array $options) {}
            }
            function f(): void {
                new Logger(['severity' => Logger::OFF]);
                new Logger(['severity' => Logger::EMERGENCY]);
            }
        