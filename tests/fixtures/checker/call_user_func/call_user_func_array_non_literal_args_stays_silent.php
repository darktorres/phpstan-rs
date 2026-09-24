<?php
            function needsInt(int $a): void {}
            function test(array $args): void {
                call_user_func_array('needsInt', $args);
            }
        