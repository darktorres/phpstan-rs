<?php
            trait Common {
                public function check(): void {
                    if (self::FLAG) {
                        echo 1;
                    }
                }
            }
            class HostConstant {
                use Common;
                const FLAG = true;
            }
            class HostUndecided {
                use Common;
                /** @var bool */
                const FLAG = PHP_INT_SIZE > 4;
            }
        