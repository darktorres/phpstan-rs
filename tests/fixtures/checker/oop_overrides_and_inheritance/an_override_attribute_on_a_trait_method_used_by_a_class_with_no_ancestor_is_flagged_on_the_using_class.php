<?php
            trait T {
                #[\Override]
                public function foo(): void {}
            }
            class Standalone {
                use T;
            }
        