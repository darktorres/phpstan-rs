<?php
            function f($ch): int {
                return curl_getinfo($ch, CURLINFO_RESPONSE_CODE);
            }
        