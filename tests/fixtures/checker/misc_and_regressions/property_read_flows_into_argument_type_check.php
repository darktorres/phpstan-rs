<?php
            class Box { public string $label = ""; }
            function needsInt(int $x): int { return $x; }
            $box = new Box();
            needsInt($box->label);
        