<?php
            function needsTwo(int $a, int $b): void {}
            function test(): void {
                call_user_func('needsTwo', 1);
            }
        