<?php
            trait Broken {
                public function boom(): int { return thisFunctionDoesNotExist(); }
            }
            class A { use Broken; }
            class B { use Broken; }
        