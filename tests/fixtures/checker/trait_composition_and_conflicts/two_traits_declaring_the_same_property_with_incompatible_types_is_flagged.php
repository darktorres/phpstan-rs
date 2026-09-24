<?php
            trait T1a { public int $x = 1; }
            trait T1b { public string $x = "a"; }
            class C1 { use T1a, T1b; }
        