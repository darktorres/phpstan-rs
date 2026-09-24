<?php
            function test(mixed $flags): void {
                $flags | 'x';
                'x' | $flags;
                $flags & 1;
            }
        