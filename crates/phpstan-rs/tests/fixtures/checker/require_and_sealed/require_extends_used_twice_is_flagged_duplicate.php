<?php
            class Base {}
            class Other {}
            /**
             * @phpstan-require-extends Base
             * @phpstan-require-extends Other
             */
            trait T {}
        