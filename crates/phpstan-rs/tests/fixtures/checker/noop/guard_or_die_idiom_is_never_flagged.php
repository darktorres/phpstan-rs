<?php
            class Guard {
                public function die_(string $msg): void {
                    exit($msg);
                }
            }
            function test(bool $cond, Guard $g): void {
                $cond || $g->die_('nope');
            }
        