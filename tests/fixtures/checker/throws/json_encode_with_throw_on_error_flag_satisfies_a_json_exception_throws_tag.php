<?php
            /** @throws \JsonException */
            function f($x): void {
                json_encode($x, JSON_THROW_ON_ERROR);
            }
        