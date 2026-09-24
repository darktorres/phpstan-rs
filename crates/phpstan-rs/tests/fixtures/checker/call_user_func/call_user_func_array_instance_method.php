<?php
            class C {
                public function needsInt(int $a): void {}
            }
            function test(C $c): void {
                call_user_func_array([$c, 'needsInt'], ["not an int"]);
            }
        