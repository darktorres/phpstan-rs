<?php
            class Base {}
            /** @phpstan-require-extends Base */
            interface Marker {}
            class Bad implements Marker {}
        