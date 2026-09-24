<?php
            /** @return resource|false */
            function open() {}
            function f(): void {
                $proc = open();
                if (! is_resource($proc)) {
                    throw new Exception('failed');
                }
                fclose($proc);
            }
        