<?php
            function f($x): string {
                return json_encode($x, JSON_HEX_TAG | JSON_HEX_AMP | JSON_THROW_ON_ERROR);
            }
        