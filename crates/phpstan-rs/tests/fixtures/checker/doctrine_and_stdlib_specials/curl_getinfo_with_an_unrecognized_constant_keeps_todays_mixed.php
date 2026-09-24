<?php
            function needsString(string $s): void {}
            function f($ch): void {
                needsString(curl_getinfo($ch, CURLINFO_NOT_IN_THE_CURATED_TABLE));
            }
        