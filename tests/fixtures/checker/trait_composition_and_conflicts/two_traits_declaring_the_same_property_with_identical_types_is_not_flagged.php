<?php
            trait T3a { public int $y = 1; }
            trait T3b { public int $y = 2; }
            class C3 { use T3a, T3b; }
        