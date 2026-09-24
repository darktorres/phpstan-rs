<?php
            function f($rewritten): string {
                $rewritten = is_string($rewritten) ? $rewritten : '';
                $rewritten = str_replace('//', '/', $rewritten);
                return strtoupper($rewritten);
            }
        