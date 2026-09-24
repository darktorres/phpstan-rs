<?php
            function needsInt(int $a): void {}
            function test(array $rest): void {
                call_user_func('needsInt', ...$rest);
            }
        