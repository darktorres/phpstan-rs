<?php
            class Ok1 { public function open(): int { return 1; } }
            class Ok2 extends Ok1 { public function open(): int { return 2; } }
        