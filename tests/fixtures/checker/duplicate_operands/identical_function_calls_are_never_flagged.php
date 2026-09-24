<?php
            function rand_(): int { return 4; }
            function f(): bool {
                return rand_() === rand_();
            }
        