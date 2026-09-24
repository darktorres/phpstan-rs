<?php
            function f(): bool {
                $arr = ['k' => 0];
                $arr['k']++;
                return $arr['k'] == 1;
            }
        