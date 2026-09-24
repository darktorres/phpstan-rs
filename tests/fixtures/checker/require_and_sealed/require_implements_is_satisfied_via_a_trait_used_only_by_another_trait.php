<?php
            interface Contract {}
            /** @phpstan-require-implements Contract */
            trait Inner {}
            trait Outer { use Inner; }
            class Good implements Contract { use Outer; }
            class Bad { use Outer; }
        