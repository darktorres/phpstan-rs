<?php
        readonly class Frozen {}
        function make(): object {
            return new readonly class {};
        }
    