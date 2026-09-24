<?php
            /**
             * @param 'r'|'w' $mode
             * @return array{0: 'pipe', 1: 'r'|'w'}|array{0: 'file', 1: non-empty-string, 2: 'r'|'w'}|resource
             */
            function descriptor(bool $piped, bool $tty, string $mode): mixed {
                if ($piped) {
                    return ['pipe', $mode];
                }
                if ($tty) {
                    return ['file', '/dev/tty', $mode];
                }
                return STDIN;
            }
        