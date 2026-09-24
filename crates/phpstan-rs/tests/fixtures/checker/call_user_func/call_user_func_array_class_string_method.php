<?php
            class C {
                public function needsInt(int $a): void {}
            }
            function test(): void {
                call_user_func_array([C::class, 'needsInt'], ["not an int"]);
            }
        