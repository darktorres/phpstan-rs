<?php
            function f($ch): int|false {
                return curl_getinfo($ch, CURLINFO_USED_PROXY);
            }
        