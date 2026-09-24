<?php
            function needsInt(int $a): void {}
            function test(): void {
                call_user_func('needsInt', 5);
            }
        