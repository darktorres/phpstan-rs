<?php
            function needsString(string $s): void {}
            function f($ch): void {
                needsString(curl_getinfo($ch));
            }
        