<?php
            /**
             * @return list<array<string, string>>
             */
            function f($ch) {
                return curl_getinfo($ch, CURLINFO_CERTINFO);
            }
        