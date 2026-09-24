<?php
            interface Contract {}
            /** @phpstan-require-implements Contract */
            trait Helper {}
            class Good implements Contract { use Helper; }
        