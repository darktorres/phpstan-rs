<?php
            /** @return resource|false */
            function open() {}
            function f(): void {
                $fp = open();
                if (! (bool) $fp) {
                    throw new Exception('failed');
                }
                fclose($fp);
            }
        