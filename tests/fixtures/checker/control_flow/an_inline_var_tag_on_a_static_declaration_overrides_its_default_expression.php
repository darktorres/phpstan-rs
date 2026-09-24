<?php
            function marker(): string {
                /** @var string|null */
                static $marker = null;
                return $marker ??= 'x';
            }
        