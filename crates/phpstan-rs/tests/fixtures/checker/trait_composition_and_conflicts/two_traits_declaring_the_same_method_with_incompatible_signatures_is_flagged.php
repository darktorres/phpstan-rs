<?php
            trait T2a { public function foo(): int { return 1; } }
            trait T2b { public function foo(): string { return "a"; } }
            class C2 { use T2a, T2b; }
        