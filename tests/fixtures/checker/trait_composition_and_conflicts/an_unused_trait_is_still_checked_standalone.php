<?php
            trait Orphaned {
                public function boom(): int { return thisFunctionDoesNotExist(); }
            }
        