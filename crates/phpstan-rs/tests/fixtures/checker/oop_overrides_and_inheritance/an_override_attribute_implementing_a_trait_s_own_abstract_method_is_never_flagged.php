<?php
            trait T {
                abstract protected function twist(): int;
            }
            class C {
                use T;
                #[\Override]
                protected function twist(): int { return 1; }
            }
        