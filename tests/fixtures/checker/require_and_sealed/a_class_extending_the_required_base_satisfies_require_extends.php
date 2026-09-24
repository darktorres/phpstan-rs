<?php
            class Base {}
            /** @phpstan-require-extends Base */
            interface Marker {}
            class Good extends Base implements Marker {}
        