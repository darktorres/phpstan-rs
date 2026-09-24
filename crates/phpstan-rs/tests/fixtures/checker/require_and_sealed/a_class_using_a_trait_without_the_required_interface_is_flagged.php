<?php
            interface Contract {}
            /** @phpstan-require-implements Contract */
            trait Helper {}
            class Bad { use Helper; }
        