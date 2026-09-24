<?php
            class Base {}
            /** @phpstan-require-extends Base */
            trait Helper {}
            class Bad { use Helper; }
        