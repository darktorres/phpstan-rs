<?php
            class Base {}
            /** @phpstan-require-extends Base */
            interface Marker {}
            interface SubMarker extends Marker {}
            class Bad implements SubMarker {}
        