<?php
            /** @phpstan-sealed Allowed */
            interface Sealed {}
            class Allowed implements Sealed {}
            class NotAllowed implements Sealed {}
        